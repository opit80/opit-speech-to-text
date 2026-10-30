# Paste methods

Dictation insertion uses the global `paste` settings. Existing configuration files
load with the same insertion behavior; no schema migration is required.

## UI contract

The complete default `paste` object in `config.json` is:

```json
{
  "paste": {
    "method": "ctrl_v",
    "after": "none",
    "restore_clipboard": false,
    "trailing_space": true
  }
}
```

| Field | JSON type | Default | Allowed values |
| --- | --- | --- | --- |
| `paste.method` (new) | string | `"ctrl_v"` | `"ctrl_v"`, `"shift_insert"`, `"ctrl_shift_v"`, `"type"` |
| `paste.after` (new) | string | `"none"` | `"none"`, `"enter"`, `"ctrl_enter"` |
| `paste.restore_clipboard` (unchanged) | boolean | `false` | `false`, `true` |
| `paste.trailing_space` (unchanged) | boolean | `true` | `false`, `true` |

Missing fields use their defaults independently, including in older partial `paste`
objects. Enum strings must exactly match the lowercase snake_case values above;
unknown values and incorrect JSON types are rejected. Both new choices are
independent; there are no new hotkey conflict rules or validation ranges.

There are no new commands, arguments, error codes, or event names. Existing
`get_config()` and `save_config({ config: AppConfig })` return the complete
`AppConfig`, including both new fields. The existing `config-changed` event also
carries the complete saved `AppConfig` with those fields. Existing command error
handling remains unchanged. Insertion failures still use the existing
`dictation-status` error state with `kind: "paste"`; no transcript is added to events
or errors by this feature.

The Settings UI should expose both enum choices and retain the existing clipboard
and trailing-space settings. Clipboard restoration has no effect during successful
`type` insertion; preserve the saved preference when changing methods. `after` may
submit a form, send a message, or execute a terminal command, so label the two Enter
choices clearly. Changing settings affects subsequent recordings/requests.

## Insertion behavior

- `ctrl_v`, `shift_insert`, and `ctrl_shift_v` write private clipboard text and send
  the corresponding shortcut. Optional clipboard restoration waits 400 ms after
  insertion, restores only an unchanged clipboard, and completes before the next
  queued job begins.
- `type` sends Unicode UTF-16 units with a down/up pair per unit. Surrogate pairs
  stay in the same chunk. CRLF, LF, and lone CR each produce one Enter press.
  Chunks contain at most 64 input events, with a 5 ms pause between chunks. The
  existing blocking paste task handles the entire insertion; successful typing
  never opens, reads, writes, or restores the clipboard.
- All methods keep the existing clipboard-only safeguards for disabled auto-paste,
  shell windows, the app's own windows, and elevated or unknown-integrity targets.
  If modifiers remain held after the one-second release wait, every method copies
  the text to the clipboard and returns clipboard-only without sending input or
  restoring the previous clipboard. No after-key is sent in this case.
  A refused `type` insertion copies the complete text to the clipboard without
  restoring it. Stops from Home/tray keep their existing clipboard-only policy.
- `after` is sent only after every insertion event was accepted, before clipboard
  restoration. No after-key is sent for clipboard-only or failed insertion. A
  failure delivering the after-key leaves the insertion successful, avoiding a
  duplicate text retry, and logs only a static diagnostic.
- Partial typing reports a sanitized failure and sends no after-key. It does not
  copy or retry partially inserted text. Long typing also stops if focus changes
  to an unsafe target between chunks. Input acceptance by Windows cannot prove
  that a target application consumed the text.

The controller resolves `effective_paste` once at recording/request start and
carries that snapshot through the queue. This is the extension point for future
foreground-app overrides. Dictation trailing space follows that snapshot for all
methods. Ask answers with `ask.output: "paste"` use the same method/after choices,
without a trailing space. Ask Copy output and History retries remain clipboard-only
and never send an after-key or restore the clipboard. In-memory dictation retries
use current settings, as before. CLI text-return and file-result flows retain their
existing behavior and do not auto-insert text.

## Desktop acceptance checks

Automated tests use fake platform effects and pure Windows INPUT builders. Complete
these checks on a real desktop without sharing transcript or clipboard contents:

1. Try all clipboard shortcuts in supported editors and terminals, with and without
   trailing space and clipboard restoration. Verify an intervening user copy is
   preserved and mixed queued jobs insert in recording order.
2. Type Turkish letters, emoji, mixed CR/LF/CRLF lines, and a long document. Check
   complete insertion, newline counts, no stuck modifiers, and an unchanged
   clipboard (including non-text formats).
3. For each method, test own-window/tray stops, shell focus, and an elevated target.
   Confirm clipboard fallback and no synthesized insertion or after-key.
4. Test each after choice in an editor and a disposable terminal session. Confirm
   exactly one after-key follows insertion, including before clipboard restoration.
5. Change method/after while recording or waiting in the queue; verify each job uses
   its starting choices. Test Ask Paste, Ask Copy, and History retries with `type`
   and an after-key selected.
6. During a long typed insertion, switch to the app's own window or an elevated
   target; verify interruption without an after-key or a duplicate clipboard paste.
