# Supported media formats

Pinpoint deliberately supports a small, portable video set that is decoded by
the production GNOME Flatpak runtime. Host-installed GStreamer plugins are not
visible inside the sandbox and are not part of this promise. GNOME Platform 50
inherits the Freedesktop 25.08 `codecs-extra` extension, which Flatpak installs
automatically with applications unless patented codecs have been explicitly
masked. H.264 support uses that declared runtime extension; AV1, VP8, VP9,
Theora, GIF, Opus, and Vorbis use base-runtime components.

For new presentations, prefer **WebM with AV1 Main or VP9 Profile 0 video and
Opus audio**. VP9 is used by the bundled introduction; both codecs have a
base-runtime software decoder and opportunistic hardware decoding on supported
systems.

## Deliberately supported set

| Filename | Container | Video | Optional audio |
|---|---|---|---|
| `.webm` | WebM | VP8, VP9 Profile 0, or AV1 Main | Vorbis or Opus |
| `.mp4` | ISO MP4 | H.264 Constrained Baseline, Main, or High; or AV1 Main | AAC-LC |
| `.mov` | QuickTime | H.264 Constrained Baseline, Main, or High | AAC-LC |
| `.ogv`, `.ogg` | Ogg | Theora | Vorbis |
| `.gif` | GIF | Animated GIF | None |

Supported conventional video is progressive, 8-bit, 4:2:0 SDR with BT.601 or
BT.709 colour signalling. Mono and stereo audio are covered. Pinpoint imposes
no separate resolution, frame-rate, or bitrate ceiling, but a presentation
must still fit the decoder, GPU, memory, and display resources of the machine
on which it is shown.

Hardware decoding is opportunistic. The same files must remain decodable by
the GNOME runtime's software elements when a suitable hardware decoder or
driver is unavailable. Pinpoint's H.264 support guarantee assumes the standard
automatically installed `codecs-extra` extension. Installations that mask or
remove it are unsupported for H.264; use AV1 or VP9 WebM when that extension
cannot be assumed.

## Common 2026 inputs outside the contract

The following are reasonable files for someone to try, but they are not part
of Pinpoint's portable playback promise:

| Likely input | What Pinpoint does | Recommended preparation |
|---|---|---|
| HEVC/H.265 in `.mp4` or `.mov` | Recognizes the container, but does not guarantee the decoder, bit depth, colour space, or HDR path | Export as progressive 8-bit 4:2:0 SDR H.264 with AAC-LC |
| 10/12-bit or non-Main-profile AV1, or AV1 in `.mkv` | The 8-bit 4:2:0 Main profile is supported in WebM and MP4; other profiles and Matroska are not release-tested | Export as 8-bit 4:2:0 AV1 Main in WebM/MP4, VP9 Profile 0 in WebM, or H.264 in MP4 |
| Apple ProRes or ProRes RAW in `.mov` | Recognizes `.mov`, but the codec and common HDR/Log variants are not guaranteed | Make a presentation copy in H.264/AAC MP4 or VP9/Opus WebM |
| OBS and other screen recordings in `.mkv` | Hands the file to GStreamer, but Matroska and its many possible codec/audio combinations are not release-tested | Remux H.264/AAC to MP4 without re-encoding, or transcode other combinations to a supported set |
| MP4 media named `.m4v` | Recognizes it as video, but only the MP4 codec combinations above are guaranteed | Keep it as `.m4v` when it contains a supported combination; otherwise transcode |
| Camera or production media named `.ts`, `.mts`, `.m2ts`, or `.mxf` | Recognizes it as video and hands it to GStreamer, but these containers and their usual codecs are not release-tested | Export or transcode a presentation copy to supported WebM or MP4 |
| 10/12-bit, HDR, 4:2:2, 4:4:4, interlaced, or alpha video in an otherwise supported container | May decode on one machine but remains outside the tested rendering contract | Export as progressive 8-bit 4:2:0 SDR with BT.601 or BT.709 signalling |

Changing a container alone does not change its codecs. For example, remuxing
HEVC from MKV to MP4 still leaves an unsupported HEVC stream; it must be
transcoded. Conversely, a normal H.264/AAC recording can often be remuxed from
MKV to MP4 quickly and without quality loss.

## Compatibility pass-throughs

For compatibility with Pinpoint 0.1.8, `.avi`, `.mpg`, `.mpeg`, `.flv`, `.wmv`,
`.mkv`, and `.3gp` filenames are still classified as video and handed to
GStreamer. They may work with a particular runtime and stream combination, but
they are not portable presentation formats and are not release-tested.

Likewise, installed decoders may make HEVC/H.265, unsupported AV1 profiles,
MPEG-2, MPEG-4 Part 2, VC-1, legacy audio, 10/12-bit video, 4:2:2 or 4:4:4
chroma, HDR, interlacing, alpha video, unusual AAC profiles, or multichannel
layouts happen to work. These remain outside the support promise until they
have representative fixtures and a clear cross-machine use case.

## How backgrounds are recognized

For an existing local asset, Pinpoint reads a small prefix and asks GLib's
shared-mime-info database whether it is video, SVG, or another image. This
allows extensionless media and avoids relying only on a fixed filename list.
For a missing or not-yet-created asset it falls back to the filename's system
content type, then to a case-insensitive compatibility list.

The fallback retains every 0.1.8 video suffix and additionally recognizes
`.m4v`, `.ts`, `.mts`, `.m2ts`, `.m2v`, `.mxf`, `.vob`, and `.m3u8`. Recognition
only selects the video pipeline; it does not expand the portable codec and
container contract above.

## Failure behaviour

An unsupported, corrupt, missing, or inaccessible video does not abort the
presentation. Pinpoint logs one warning, keeps the slide text and stage colour
visible, stops the failed pipeline, and caches the failure so it does not retry
on every frame. A pipeline that has not prepared within 20 seconds is cancelled
and treated the same way. PDF export similarly retains the slide and omits a
thumbnail when a video cannot be decoded.

SVG input is limited to 16 MiB, 100,000 elements, and 256 levels of nesting.
Document type declarations are rejected to prevent entity-expansion bombs, and
loading is cancelled after 10 seconds. librsvg's own bounded XML parser remains
enabled. A rejected SVG leaves the slide text and stage colour visible.

## Automated contract

`tests/test-media-formats.c` checks the production contract against synthetic
fixtures in `tests/fixtures/media-formats/` and the bundled VP9/Opus video. It:

- verifies every required demuxer and software decoder in the production
  runtime, including the automatically installed codec extension used by
  H.264;
- discovers the expected container, codecs, H.264 and AV1 profiles, AAC-LC
  profile, 8-bit 4:2:0 layout, and BT.601/BT.709 signalling;
- decodes every supported fixture to raw video and, where present, raw audio;
- verifies deterministic rejection of a corrupt MP4 fixture.

The fixtures use generated GStreamer test patterns and sine-wave audio. The
H.264 encoder used to create the files is optional. AV1 playback is exercised
through the base runtime's dav1d software decoder, independently of hardware
decoding and optional codec extensions.
