//! GNOME custom keybindings. Never replace the list with only our own paths.

use super::*;
use gtk::glib::variant::ToVariant;
use gtk::glib::{Variant, VariantTy};

const SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
const CUSTOM_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";

fn path(action: &str) -> String {
    format!(
        "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/opit-speech-to-text/{}/",
        fingerprint(action)
    )
}
fn string_value(text: &str) -> Result<String, &'static str> {
    Variant::parse(Some(VariantTy::STRING), text)
        .ok()
        .and_then(|value| value.get::<String>())
        .ok_or("GNOME shortcut registry returned an invalid string")
}
fn list_value(text: &str) -> Result<Vec<String>, &'static str> {
    Variant::parse(Some(VariantTy::STRING_ARRAY), text)
        .ok()
        .and_then(|value| value.get::<Vec<String>>())
        .ok_or("GNOME shortcut registry returned an invalid list")
}
fn edit_list(mut list: Vec<String>, own: &str, install: bool) -> Vec<String> {
    if install {
        if !list.iter().any(|entry| entry == own) {
            list.push(own.into());
        }
    } else {
        list.retain(|entry| entry != own);
    }
    list
}
fn read_list() -> Result<Vec<String>, &'static str> {
    list_value(&helper("gsettings", &["get", SCHEMA, "custom-keybindings"])?)
}
fn read(schema: &str, key: &str) -> Result<String, &'static str> {
    string_value(&helper("gsettings", &["get", schema, key])?)
}
fn write(schema: &str, key: &str, value: &str) -> Result<(), &'static str> {
    helper("gsettings", &["set", schema, key, &value.to_variant().print(true)]).map(|_| ())
}
fn write_list(own: &str, install: bool) -> Result<(), &'static str> {
    // Re-read immediately before editing, preserving entries added by other applications.
    let edited = edit_list(read_list()?, own, install);
    helper("gsettings", &["set", SCHEMA, "custom-keybindings", &edited.to_variant().print(true)])?;
    if read_list()? != edited {
        return Err("GNOME shortcut list changed during registration; refresh and retry");
    }
    Ok(())
}

pub(super) fn manage(
    action: &str,
    combo: Option<&Combo>,
    operation: Operation,
    state: &mut RegistryState,
) -> Result<(), &'static str> {
    let path = path(action);
    let schema = format!("{CUSTOM_SCHEMA}:{path}");
    let list = read_list()?;
    let listed = list.contains(&path);
    let name = format!("Opit Speech to Text ({action})");
    // Do not overwrite a foreign entry, even if somebody manually reused our stable path.
    let existing_name = read(&schema, "name")?;
    if !existing_name.is_empty() && existing_name != name {
        return Err("GNOME shortcut path belongs to another application");
    }
    let existing_command = read(&schema, "command")?;
    let existing_binding = read(&schema, "binding")?;
    if existing_name.is_empty() && (!existing_command.is_empty() || !existing_binding.is_empty()) {
        return Err("GNOME shortcut path belongs to another application");
    }
    state.owned = existing_name == name || listed;
    state.command_matches = existing_command == state.command;
    let expected = combo.map(Combo::gnome);
    // Matching entries need no conflict scan or write. Before changing a binding,
    // preserve custom bindings owned by any other application, including other actions.
    if !matches!(operation, Operation::Remove)
        && let Some(binding) = &expected
        && (!listed || existing_binding != *binding || !state.command_matches)
    {
        for other in list.iter().filter(|other| **other != path) {
            if read(&format!("{CUSTOM_SCHEMA}:{other}"), "binding")? == *binding {
                state.conflict = true;
                break;
            }
        }
    }
    if state.conflict && matches!(operation, Operation::Install) {
        return Ok(());
    }
    match operation {
        Operation::Install => {
            let binding = combo.ok_or("Invalid Linux shortcut")?.gnome();
            // Remove the previous binding before changing it. Other paths stay untouched.
            write(&schema, "binding", "")?;
            write(&schema, "name", &name)?;
            write(&schema, "command", &state.command)?;
            write(&schema, "binding", &binding)?;
            write_list(&path, true)?;
        }
        Operation::Remove => {
            // Disable before removing the path. Reset only our three keys, never a schema subtree.
            write(&schema, "binding", "")?;
            write_list(&path, false)?;
            for key in ["name", "command", "binding"] {
                helper("gsettings", &["reset", &schema, key])?;
            }
            return Ok(());
        }
        Operation::Read => {}
    }
    if listed || matches!(operation, Operation::Install) {
        let binding = read(&schema, "binding")?;
        if !binding.is_empty() {
            state.registered = Some(binding);
        }
        if matches!(operation, Operation::Install)
            && (state.registered.as_deref() != combo.map(Combo::gnome).as_deref()
                || read(&schema, "command")? != state.command)
        {
            return Err("GNOME shortcut registration readback failed");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn list_edits_preserve_every_foreign_entry_order_and_escaping() {
        let original = vec!["/other/a/".into(), "/other/quote's/".into(), "/other/back\\slash/".into()];
        let own = path("dictation");
        let installed = edit_list(original.clone(), &own, true);
        assert_eq!(&installed[..3], &original);
        assert_eq!(edit_list(installed.clone(), &own, true), installed);
        assert_eq!(edit_list(installed.clone(), &own, false), original);
        assert_eq!(list_value(&installed.to_variant().print(true)).unwrap(), installed);
        assert_eq!(list_value("@as []").unwrap(), Vec::<String>::new());
        assert!(list_value("'not an array'").is_err());
    }
    #[test]
    fn gvariant_strings_roundtrip_commands_without_shell_evaluation() {
        let command = "'/tmp/Opit'\\''s $app' --shortcut 'dictation'";
        assert_eq!(string_value(&command.to_variant().print(true)).unwrap(), command);
        assert_ne!(path("dictation"), path("pause"));
        assert_ne!(path("style:formal"), path("style:other"));
    }
}
