# Local transcription in Opit Speech to Text

Local transcription is an optional feature of Opit Speech to Text. After the initial
downloads it needs neither an API key nor an internet connection.

## Setup

1. In Profiles, add **Local (offline)**. The same option is available in the setup wizard.
2. Download the default CPU / OpenBLAS server and one model. Every download is checked
   against an exact byte size and SHA-256 embedded in the app. Unverified downloads are
   deleted. Cancel removes the partial download; a new attempt starts from zero.
3. Save the profile and make it active. Use Test connection to load the model now,
   or begin dictating to load it on demand. The overlay shows that the model is loading.

The model list marks a recommendation using total RAM and, when available, NVIDIA VRAM.
It is a starting point, not a performance measurement. With at least 16 GiB RAM, or
8 GiB RAM plus 4 GiB VRAM, the recommended model is `large-v3-turbo-q5_0`. It is also
the default local profile model and a useful starting point for Turkish. With less
RAM, the recommendation becomes small, base, or tiny.

| Model file | Download bytes | Approximate decimal MB |
|---|---:|---:|
| ggml-tiny-q5_1.bin | 32,152,673 | 32.2 |
| ggml-base-q5_1.bin | 59,707,625 | 59.7 |
| ggml-base.bin | 147,951,465 | 148.0 |
| ggml-small-q5_1.bin | 190,085,487 | 190.1 |
| ggml-small.bin | 487,601,967 | 487.6 |
| ggml-medium-q5_0.bin | 539,212,467 | 539.2 |
| ggml-large-v3-turbo-q5_0.bin | 574,041,195 | 574.0 |
| ggml-large-v3-turbo-q8_0.bin | 874,188,075 | 874.2 |
| ggml-large-v3-turbo.bin | 1,624,555,275 | 1,624.6 |
| ggml-large-v3-q5_0.bin | 1,081,140,203 | 1,081.1 |

The pinned server release is `b5454`. Compressed server downloads are 21,715,607 bytes
for CPU / OpenBLAS, 8,928,640 for compatibility CPU, 684,913,404 for CUDA 12.4, and
284,761,231 for CUDA 11.8. Extraction needs additional disk space. The installer
searches the extracted tree for `whisper-server.exe` and retains adjacent DLLs.
A build without that executable fails installation explicitly.

## Performance and GPU

The first request includes loading model weights. Smaller models usually load faster,
use less RAM, and finish sooner; larger models can improve accuracy but require more
memory and compute. Quantized variants reduce download and memory use. Actual latency
depends on your CPU, GPU, model, recording length and other running applications.
There are no measured speed guarantees for these builds on this PC.

CPU threads defaults to 0, selecting approximately one per physical core. GPU is off
by default. On a detected NVIDIA system the build picker also offers CUDA 12.4 and
CUDA 11.8. Download and select the appropriate CUDA build, then enable GPU. Driver
compatibility is not inferred from the GPU name; try CUDA 11.8 for an older driver,
or use CPU if startup fails. CPU-only builds remain available as a fallback.

Readiness has a three-minute timeout. An unexpectedly terminated server is restarted
once; repeated crashes fail rather than creating an endless process loop. A free
ephemeral port is chosen for each launch and the server binds only to `127.0.0.1`.
The default idle timeout is ten minutes; 0 keeps it loaded. Active requests prevent
idle shutdown. Optional preload loads the active local profile at app startup.
Exiting the app stops its server. On Windows, a kill-on-close Job Object also stops
the server if the app crashes.

## Storage and removal

By default, binaries and models live under
`%LOCALAPPDATA%\opit-speech-to-text\local\bin` and `local\models`.
Under local performance and storage, Models folder accepts an absolute ASCII path
such as `C:\OpitModels`. The app manages only the dedicated
`opit-speech-to-text-local` child inside that folder, with `bin` and `models` children.
Use this override if your Windows profile contains non-ASCII characters: the local
engine may not be able to open those paths. Changing the folder does not move existing
downloads. Delete old downloads before changing it, or remove the old dedicated
folder yourself after exiting the app.

`OPIT_DATA_DIR` takes precedence over the models folder setting. With that variable
set, all local downloads go under `<OPIT_DATA_DIR>\local`, alongside the throwaway
configuration and history. Tests must use an ASCII scratch path.

Delete model and Delete server remove the selected artifact and stop the local server
first. Uninstalling with Delete application data also removes the default local
folder and the currently configured custom dedicated folder. Updates and silent
reinstalls preserve these files. Previously configured custom folders must be
removed manually.

## Privacy and fallbacks

The local profile has no API key. Only the application downloads artifacts; the UI
does not perform network requests. Local transcription uses a keyless loopback HTTP
request. Detected language and segment timing flow through the same history and file
transcription paths as other profiles.

You may set a cloud profile as fallback for a local profile, or a local profile as
fallback for a cloud profile. Cloud fallback sends audio to that provider and needs
its key and internet access. Optional AI cleanup and Ask mode retain their separately
configured network behavior; local transcription does not make them offline. AI
cleanup is disabled in the local preset. For fully offline use, leave cloud fallback
and AI cleanup disabled.

Automated checks use synthetic HTTP responses, archives and clocks. Real artifact
downloads, GPU startup, microphone dictation and transcription speed require a
separate manual pass on the target PC.
