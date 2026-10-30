# Recording queue and pause in Opit Speech to Text

You can start a new recording with the recording shortcut, Home, or the tray while
an earlier dictation is being processed. Push-to-talk also works while processing.
Opit Speech to Text accepts up to three waiting recordings in addition to the item
being processed. When the queue is full, the overlay explains why recording cannot
start. Home and the overlay show the number of waiting items.

Transcription and paste run one item at a time in recording order. Each paste,
including optional clipboard restoration, finishes before the next item starts.
This keeps later, faster requests from overtaking earlier recordings. Recording
continues independently. Each item retains the settings and profile selected when
its recording started. Stopping from Home or the tray always makes that item
clipboard-only, even if focus changes before its turn. Hotkey and automatic stops
also retain the clipboard-only decision when our window or a shell window had focus.
Successful results follow each item's history settings and get separate history rows.

Press Escape during recording to discard only that recording, including a paused
recording. Without a live recording, Escape cancels the processing request. Waiting
items continue. A paste already in progress cannot be cancelled. Try again is
available only after all recording and processing have finished; it resends the
last retained failed audio with the currently selected profile. Starting a new
recording or completing a successful transcription clears previously retained audio.
Updates and microphone tests are refused while recording or processing is active.
Closing the settings window leaves recording and the queue running; reopening reads
the current status. Quitting the app discards unfinished work.

To pause, open the tray menu and choose **Pause recording**. The item changes to
**Resume recording** until you resume. Paused audio is discarded, the microphone
level stays at its last value, and the recording time limit counts only active time.
Stopping a paused recording transcribes the audio captured before the pause.

You can assign an optional **Pause or resume recording shortcut** in Settings using
the same shortcut editor as the recording shortcut. **Use tray only** removes that
binding. The default is no pause shortcut (`hotkey.pause_keys: []`); existing
configuration files keep working. The pause shortcut cannot equal or contain the
recording shortcut, or be contained by it. Escape remains reserved for cancellation.
The shortcut-enabled and shortcut-pause settings apply to both shortcuts.

The automated tests use fake devices and local provider responses. A real microphone,
tray menu, focused target application, and clipboard restoration still require a
desktop acceptance pass; no live provider request is needed for automated checks.
