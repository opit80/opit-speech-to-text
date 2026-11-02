//! Windows transport. The protected DACL contains exactly one allow ACE, for the
//! process token's user SID, which also owns the pipe. Pipe instances reject remote clients
//! explicitly. Clients verify the pipe's owner and server session before sending anything.

use std::io;
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::time::Duration;

use tokio::io::BufReader;
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SE_KERNEL_OBJECT,
};
use windows::Win32::Security::{
    GetTokenInformation, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER, TokenSessionId, TokenUser,
};
use windows::Win32::System::Pipes::{GetNamedPipeServerProcessId, GetNamedPipeServerSessionId};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::core::{PCWSTR, PWSTR};

use crate::{Error, MAX_REQUEST, MAX_RESPONSE, Request, Response, VERSION, read_frame, write_frame};

fn win_error(_: windows::core::Error) -> io::Error {
    io::Error::last_os_error()
}

struct Token(HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct LocalAllocation(*mut std::ffi::c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0)));
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Identity {
    sid: String,
    session: u32,
}

fn process_identity(process: HANDLE) -> io::Result<Identity> {
    unsafe {
        let mut token = Token(HANDLE::default());
        OpenProcessToken(process, TOKEN_QUERY, &mut token.0).map_err(win_error)?;
        let mut session = 0u32;
        let mut needed = 0;
        GetTokenInformation(
            token.0,
            TokenSessionId,
            Some((&mut session as *mut u32).cast()),
            size_of::<u32>() as u32,
            &mut needed,
        )
        .map_err(win_error)?;
        let mut needed = 0;
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut needed);
        if needed == 0 {
            return Err(io::Error::last_os_error());
        }
        // usize storage supplies the alignment required by TOKEN_USER.
        let mut storage = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        GetTokenInformation(token.0, TokenUser, Some(storage.as_mut_ptr().cast()), needed, &mut needed)
            .map_err(win_error)?;
        let user = &*storage.as_ptr().cast::<TOKEN_USER>();
        Ok(Identity { sid: sid_string(user.User.Sid)?, session })
    }
}

/// # Safety
/// `sid` must point to a valid SID for the duration of the call.
unsafe fn sid_string(sid: PSID) -> io::Result<String> {
    unsafe {
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(sid, &mut text).map_err(win_error)?;
        let _allocation = LocalAllocation(text.0.cast());
        text.to_string().map_err(|_| io::Error::other("Invalid user SID."))
    }
}

fn user_sid() -> io::Result<String> {
    Ok(process_identity(unsafe { GetCurrentProcess() })?.sid)
}

pub fn pipe_name() -> io::Result<String> {
    let Identity { sid, session } = process_identity(unsafe { GetCurrentProcess() })?;
    Ok(format!(r"\\.\pipe\opit-{sid}-{session}-v1"))
}

/// Bound to the connected pipe instance itself rather than to a PID: the pipe's owner SID
/// (`create_server` sets it to the user; another user cannot assign that owner without restore
/// privileges) and the session of the server process.
fn pipe_identity(client: &NamedPipeClient) -> io::Result<Identity> {
    let unavailable = || io::Error::other("Pipe owner unavailable.");
    unsafe {
        let handle = HANDLE(client.as_raw_handle());
        let mut session = 0;
        GetNamedPipeServerSessionId(handle, &mut session).map_err(win_error)?;
        let mut owner = PSID::default();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        GetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            None,
            None,
            Some(&mut descriptor),
        )
        .ok()
        .map_err(|_| unavailable())?;
        let _allocation = LocalAllocation(descriptor.0);
        if owner.0.is_null() {
            return Err(unavailable());
        }
        Ok(Identity { sid: sid_string(owner)?, session })
    }
}

/// The pipe-bound identity, which must agree with the token of the process that
/// `GetNamedPipeServerProcessId` names.
fn server_identity(client: &NamedPipeClient) -> io::Result<Identity> {
    let pipe = pipe_identity(client)?;
    let process = unsafe {
        let mut pid = 0;
        GetNamedPipeServerProcessId(HANDLE(client.as_raw_handle()), &mut pid).map_err(win_error)?;
        // The open handle pins the process only from here on, so the token query below cannot
        // race a later PID reuse. The PID may already have been reused before OpenProcess, so
        // this check alone proves nothing about the pipe; the pipe-bound identity above does.
        let process = Token(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(win_error)?);
        process_identity(process.0)?
    };
    if pipe != process {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Pipe and server process disagree."));
    }
    Ok(pipe)
}

fn authenticate(identity: io::Result<Identity>, expected: &Identity) -> io::Result<()> {
    if identity? != *expected {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Untrusted local control server."));
    }
    Ok(())
}

/// The user owns the pipe (also when the app runs elevated, whose default owner would be the
/// Administrators group) and is the only principal with access.
fn sddl(sid: &str) -> String {
    format!("O:{sid}D:P(A;;GA;;;{sid})")
}

/// `first` prevents silently taking over an existing server's name. Keep one
/// listening instance alive while connected requests are dispatched concurrently.
pub fn create_server(name: &str, first: bool) -> io::Result<NamedPipeServer> {
    let descriptor: Vec<u16> = sddl(&user_sid()?).encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut security = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(descriptor.as_ptr()), 1, &mut security, None)
            .map_err(win_error)?;
        let _allocation = LocalAllocation(security.0);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: security.0,
            bInheritHandle: false.into(),
        };
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .max_instances(16)
            .create_with_security_attributes_raw(name, (&attributes as *const SECURITY_ATTRIBUTES).cast_mut().cast())
    }
}

async fn connect(name: &str) -> io::Result<NamedPipeClient> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        // SECURITY_IDENTIFICATION prevents a pipe-name squatter from impersonating the CLI.
        match ClientOptions::new().security_qos_flags(0x0001_0000).open(name) {
            Ok(client) => return Ok(client),
            Err(error) if error.raw_os_error() == Some(231) && tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
            Err(error) => return Err(error),
        }
    }
}

pub async fn call(request: &Request) -> Result<Response, Error> {
    let name = pipe_name().map_err(|_| Error::new("transport", "Could not identify the local user session."))?;
    call_at(&name, request).await
}

pub async fn call_at(name: &str, request: &Request) -> Result<Response, Error> {
    let expected = process_identity(unsafe { GetCurrentProcess() })
        .map_err(|_| Error::new("transport", "Could not identify the local user session."))?;
    call_at_for_identity(name, request, &expected).await
}

async fn call_at_for_identity(name: &str, request: &Request, expected: &Identity) -> Result<Response, Error> {
    request.validate()?;
    let mut client = connect(name).await.map_err(|error| {
        if error.raw_os_error() == Some(2) {
            Error::new("app_not_running", "Opit Speech to Text is not running in this user session.")
        } else {
            Error::new("transport", "Could not connect to Opit Speech to Text.")
        }
    })?;
    authenticate(server_identity(&client), expected)
        .map_err(|_| Error::new("untrusted_server", "Could not authenticate the local control server."))?;
    write_frame(&mut client, request, MAX_REQUEST).await?;
    // File jobs may be long. Bound dead peers without retrying mutations.
    let bytes = tokio::time::timeout(Duration::from_secs(7200), read_frame(&mut BufReader::new(client), MAX_RESPONSE))
        .await
        .map_err(|_| Error::new("timeout", "Local operation timed out; inspect status before retrying."))??;
    let response: Response =
        serde_json::from_slice(&bytes).map_err(|_| Error::new("protocol", "Invalid local control response."))?;
    if response.version != VERSION {
        return Err(Error::new("protocol_version", "Unsupported local control protocol version."));
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_identity_verification_errors_fail_closed() {
        let expected = process_identity(unsafe { GetCurrentProcess() }).unwrap();
        assert!(authenticate(Err(io::Error::other("token unavailable")), &expected).is_err());
    }

    #[tokio::test]
    async fn impostor_server_receives_no_request_for_a_foreign_sid_or_session() {
        use tokio::io::AsyncReadExt;
        for foreign_session in [false, true] {
            let mut expected = process_identity(unsafe { GetCurrentProcess() }).unwrap();
            if foreign_session {
                expected.session += 1;
            } else {
                expected.sid = "S-1-5-21-0-0-0-1234".into();
            }
            let name = format!("{}-impostor-{}-{foreign_session}", pipe_name().unwrap(), std::process::id());
            let mut server = create_server(&name, true).unwrap();
            let received = tokio::spawn(async move {
                server.connect().await.unwrap();
                let mut bytes = Vec::new();
                server.read_to_end(&mut bytes).await.unwrap();
                bytes
            });
            let request = Request::new(crate::Command::Ask { text: Some("private request".into()) });
            let error = call_at_for_identity(&name, &request, &expected).await.err().expect("untrusted server");
            assert_eq!(error.code, "untrusted_server");
            assert!(tokio::time::timeout(Duration::from_secs(2), received).await.unwrap().unwrap().is_empty());
        }
    }

    #[test]
    fn descriptor_grants_only_the_user_and_name_includes_session() {
        let sid = user_sid().unwrap();
        assert_eq!(sddl(&sid), format!("O:{sid}D:P(A;;GA;;;{sid})"));
        assert!(pipe_name().unwrap().starts_with(&format!(r"\\.\pipe\opit-{sid}-")));
    }

    #[tokio::test]
    #[ignore = "Windows named pipe integration; run explicitly on a desktop/CI worker"]
    async fn private_pipe_roundtrip() {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Security::Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT};
        use windows::Win32::Security::{ACCESS_ALLOWED_ACE, DACL_SECURITY_INFORMATION, GetAce};
        let name = format!("{}-test-{}", pipe_name().unwrap(), std::process::id());
        let mut server = create_server(&name, true).unwrap();
        unsafe {
            let mut acl = std::ptr::null_mut();
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            GetSecurityInfo(
                HANDLE(server.as_raw_handle()),
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(&mut acl),
                None,
                Some(&mut descriptor),
            )
            .ok()
            .unwrap();
            let _allocation = LocalAllocation(descriptor.0);
            assert!(!acl.is_null(), "a NULL DACL would allow every user");
            assert_eq!((*acl).AceCount, 1, "only the user may have an ACE");
            let mut ace = std::ptr::null_mut();
            GetAce(acl, 0, &mut ace).unwrap();
            let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            assert_eq!(ace.Header.AceType, 0, "ACCESS_ALLOWED_ACE_TYPE");
            let mut sid = PWSTR::null();
            ConvertSidToStringSidW(
                windows::Win32::Security::PSID((&ace.SidStart as *const u32).cast_mut().cast()),
                &mut sid,
            )
            .unwrap();
            let _sid_allocation = LocalAllocation(sid.0.cast());
            assert_eq!(sid.to_string().unwrap(), user_sid().unwrap());
        }
        assert!(create_server(&name, true).is_err(), "first instance cannot be replaced");
        let task = tokio::spawn(async move {
            server.connect().await.unwrap();
            let bytes = read_frame(&mut BufReader::new(&mut server), MAX_REQUEST).await.unwrap();
            assert!(matches!(crate::decode_request(&bytes).unwrap().command, crate::Command::Status));
            write_frame(&mut server, &Response::from_result(Ok(serde_json::json!({"state":"idle"}))), MAX_RESPONSE)
                .await
                .unwrap();
        });
        let expected = process_identity(unsafe { GetCurrentProcess() }).unwrap();
        let probe_name = format!("{name}-probe");
        let probe_server = create_server(&probe_name, true).unwrap();
        let accepted = tokio::spawn(async move { probe_server.connect().await.map(|_| probe_server) });
        let probe = connect(&probe_name).await.unwrap();
        assert_eq!(pipe_identity(&probe).unwrap(), expected, "the user owns the pipe");
        assert_eq!(server_identity(&probe).unwrap(), expected);
        drop(probe);
        drop(accepted.await.unwrap());
        let response = call_at(&name, &Request::new(crate::Command::Status)).await.unwrap();
        assert!(matches!(response.outcome, crate::Outcome::Ok { .. }));
        task.await.unwrap();
    }
}
