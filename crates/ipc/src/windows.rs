//! Windows transport. The protected DACL contains exactly one allow ACE, for the
//! process token's user SID. Pipe instances reject remote clients explicitly.

use std::io;
use std::mem::size_of;
use std::time::Duration;

use tokio::io::BufReader;
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};
use windows::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId, OpenProcessToken};
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

fn user_sid() -> io::Result<String> {
    unsafe {
        let mut token = Token(HANDLE::default());
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token.0).map_err(win_error)?;
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
        let mut sid = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut sid).map_err(win_error)?;
        let _allocation = LocalAllocation(sid.0.cast());
        sid.to_string().map_err(|_| io::Error::other("Invalid user SID."))
    }
}

pub fn pipe_name() -> io::Result<String> {
    let sid = user_sid()?;
    let mut session = 0;
    unsafe {
        ProcessIdToSessionId(GetCurrentProcessId(), &mut session).map_err(win_error)?;
    }
    Ok(format!(r"\\.\pipe\opit-{sid}-{session}-v1"))
}

fn sddl(sid: &str) -> String {
    format!("D:P(A;;GA;;;{sid})")
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
    request.validate()?;
    let mut client = connect(name).await.map_err(|error| {
        if error.raw_os_error() == Some(2) {
            Error::new("app_not_running", "Opit Speech to Text is not running in this user session.")
        } else {
            Error::new("transport", "Could not connect to Opit Speech to Text.")
        }
    })?;
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
    fn descriptor_grants_only_the_user_and_name_includes_session() {
        let sid = user_sid().unwrap();
        assert_eq!(sddl(&sid), format!("D:P(A;;GA;;;{sid})"));
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
        let response = call_at(&name, &Request::new(crate::Command::Status)).await.unwrap();
        assert!(matches!(response.outcome, crate::Outcome::Ok { .. }));
        task.await.unwrap();
    }
}
