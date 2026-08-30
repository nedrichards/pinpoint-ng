# Media format fixtures

These short, synthetic fixtures define Pinpoint's deliberately supported media
families without depending on personal or externally licensed source material:

- WebM with VP8 and Vorbis
- MP4 with H.264 Constrained Baseline and AAC-LC
- QuickTime MOV with H.264 Main
- MP4 with H.264 High
- Ogg with Theora and Vorbis
- animated GIF
- WebM with AV1 Main and Opus
- MP4 with AV1 Main and AAC-LC

The bundled introduction supplies the VP9 and Opus fixture. Every encoded video
uses progressive, 8-bit, 4:2:0 SDR source at 15 frames per second. The BT.601
fixtures are 64×48; the H.264 High BT.709 fixture is 128×72. `corrupt.mp4`
exercises deterministic rejection. The automated test verifies both the
encoded stream metadata and a decoded raw preroll.

The fixtures were generated with GStreamer 1.26.11 in the GNOME 50 Flatpak
runtime. Test patterns and sine-wave audio are generated content. The H.264
encoder and decoder come from the runtime-declared, automatically installed
`codecs-extra` extension; the AV1 fixtures use the base runtime's AOM encoder.
Playback tests explicitly verify the base dav1d AV1 software decoder
independently of hardware decoders and codec extensions.
