# Tangerine's options: the reference for Quicky

This was extracted from 66 screenshots of tangerineformac.com and Tangerine's windows, taken in October 2026. Use it as a checklist for **what** Quicky offers and in **what order**. Quicky keeps its own look: Omarchy theme colours, monospace type, and the octagon wheel.

Tangerine's interaction model: start dragging, tap a key, and a wheel opens that stays open until you drop. The keys are Shift for Convert formats, Command for File tools, and Control for File actions. All three can be changed in Settings.

## Convert wheels (Shift), clockwise from the top

Tangerine leaves the source file's own format out of the convert wheel.

| Kind | Targets | Notes |
| --- | --- | --- |
| Image | PNG, WEBP, HEIC, TIFF, AVIF, BMP, PDF | Inputs also include JPG; SVG is input only. PDF is export only. |
| Video | MP4, MKV, WEBM, AVI, WMV, GIF, MP3 | Inputs also include MOV. MP3 extracts the audio. |
| Audio | M4A, WAV, FLAC, OGG, OPUS, AIFF, WMA | Inputs also include MP3. |
| PDF | DOCX, PAGES, PPTX, XLSX, JPG, PNG, EPUB, TXT, MD | All pages are converted; images come out at 300 DPI. The key is labelled "Convert PDF". |
| Documents | PAGES, PDF, TXT, MD, HTML, EPUB, PNG, JPG | RTF, RTFD and text clippings are input only; HTML is export only. There is no Tools wheel. |
| Slides | PDF, JPG, PNG | PPT is input only. There is no Tools wheel. |
| Spreadsheets | ODS, CSV, TSV, PDF | XLS is input only. There is no Tools wheel. |
| EPUB | PDF, DOCX, PAGES, TXT, MD, HTML | |
| Subtitles | VTT, TXT (from SRT) | |
| Archives | TAR, GZIP | RAR is input only. |

## File tools (Command), one file, clockwise from the top

| Kind | Tools |
| --- | --- |
| Image | Compress, Metadata, Edit, Annotate, Add BG, Crop, Redact |
| Video | Compress, Metadata, Mute, Trim, Crop, Speed, Snapshot, Split, Redact |
| Audio | Compress, Metadata, Normalize, Visualize, Trim, Channels, Bleep |
| PDF | Compress, Metadata, Split, Organize |
| Archive | Extract (captioned "Extract Archive") |
| Any file | Compress and Metadata work on anything |

## Several files

Multi-file tools carry a "Multiple files" badge on the site.

| Tool | Files | Result |
| --- | --- | --- |
| Join videos | MP4, MOV and MKV clips; you arrange the playback order | one MP4 |
| Create a PDF | images, mixed formats allowed | one PDF, one page per image |
| Make a collage | images; grid, row, column or featured layout; order, spacing, corners, background | one image |
| Merge PDFs | PDFs, in a "Document order" list | one PDF |
| Compress (images and videos) | "one image or a selection"; the originals are kept | `Compressed.jpg`, `Compressed.mov` |

## Actions (Control), clockwise from the top

AirDrop, Float, Copy, Move, Copy Path, Terminal.

## Editor windows

Every editor works on one file unless noted. The original is always kept: buttons read "Save … Copy" or "Apply", or the window creates a new file.

### Any file
- **Compress:**
  - The file name and its original size.
  - Compression preset: **Balanced** / **Strong**. Balanced shows the help text "Smaller files with less quality loss."
  - Image size: a dropdown ("Keep original dimensions").
  - A checkbox, "Compress to target file size". The target can be a file size or a percentage reduction.
  - Button: Compress.
- **Metadata:**
  - The file name and a "Search metadata" field.
  - Fields grouped by section (for example ExifIFD and File; for media, by track or chapter). Some fields are editable.
  - A ⊖ button on each field.
  - Note: "For photos, Remove All also clears hidden camera and processing data. HDR and portrait effects may change."
  - Buttons: **Remove All**, **Save**.

### Image
- **Edit Photo:**
  - A live preview.
  - Light: Exposure (EV), Brightness, Contrast, Highlights, Shadows, Whites, Blacks, Gamma.
  - Color: Temperature, Tint, Saturation, Vibrance, Hue (°).
  - Detail: Clarity, Dehaze, Grain, Noise reduction.
  - Each slider shows its value and has a ↺ reset.
  - Button: **Save Edited Copy**.
- **Annotate Photo:**
  - Tools: select/arrow, line, text (Aa), rectangle, circle, pencil, numbered label, magnifier, image.
  - A colour pill, Undo, Delete.
  - Hint: "Drag from the tail toward the arrowhead. Drag an annotation to move it."
  - Buttons: Reset, **Save Annotated Copy**.
- **Add BG:**
  - Background: a row of gradient swatches, a row of solid colours, a custom colour, or an image picker (another image as the background, optionally blurred).
  - Sliders: Padding (160 px), Corners (24 px), Shadow (24 px).
  - Ratio: Auto / 1:1 / 16:9 / 4:5.
  - Buttons: Reset, **Save with Background**.
- **Crop Image:**
  - A crop box with corner and edge handles.
  - Aspect ratio: Free / 1:1 / 16:9 / 9:16 / 4:3 / 3:4.
  - W and H fields in px.
  - Buttons: Reset, **Apply**.
- **Redact Photo:**
  - Several movable boxes.
  - Style: **Solid** / **Blur**, plus a colour.
  - Detect: **Faces**, **Text** (done locally).
  - Buttons: Reset, **Save Redacted Copy**.
- **Create Collage** (several images):
  - Style: Grid / Row / Column / Featured.
  - Columns: a stepper.
  - Canvas: a ratio (for example Square · 1:1).
  - Width: for example 2,048 px.
  - Image fit: for example "Fill cells (crop edges)".
  - Collapsible sections: "Spacing & background" and "Image order".
  - Buttons: Reset Layout, **Create Collage**.

### Video
- **Trim Video:**
  - Layout, top to bottom: preview, then "Drag the ends to trim" / "N selected", then the filmstrip with thick end handles and a playhead, then `Start 0:00.00` / `End 0:04.00`, then ◁| ▶ |▷ with `0:01.60 / 0:04.00`.
  - Button: **Apply**.
  - Timecodes can be typed precisely.
- **Crop Video:**
  - A crop box.
  - Aspect ratio: Free / 1:1 / 16:9 / 9:16 / 4:3 / 3:4.
  - W and H fields in px.
  - A scrubber with frame stepping.
  - The original audio is kept.
  - Buttons: Reset, **Apply**.
- **Change Video Speed:**
  - A slider with stops at 0.25×, 0.5×, 1×, 1.5×, 2× and 4×.
  - A play preview.
  - Button: **Apply**.
- **Join Videos** (several files):
  - A "Playback order" list. Each row shows its index, a thumbnail, the filename and ▲/▼ buttons. The first row's ▲ and the last row's ▼ are disabled.
  - Button: **Join Videos**.
- **Video Snapshots:**
  - A scrubber with frame stepping.
  - **Add Frame** collects tiles. Each tile has a timestamp badge and an × to remove it.
  - An "N selected" count.
  - Button: **Save N Frame(s)**.
- **Split Video:**
  - **Add Split** places a split point at the playhead. An "N split points" count is shown.
  - "Equal clips" takes a number with a stepper and a **Set** button.
  - The filmstrip with a playhead.
  - Button: **Split Video**, disabled while there are no split points.
- **Redact Video:**
  - Boxes, with Style Solid / Blur and a colour.
  - A "Redactions" timeline: one track per box, with draggable time ranges. Drag on an empty track to add a box.
  - Playback controls.
  - Button: **Save Redacted Copy**.

### Audio
- **Trim Audio:**
  - The waveform with handles, "N selected", Start and End.
  - A checkbox, "Trim silent ends automatically".
  - **▶ Play Selected Range**.
  - Button: **Apply**.
- **Normalize Volume:**
  - The waveform with play.
  - Target loudness −16 LUFS, Loudness range 11 LU, True peak limit −1.5 dBTP.
  - Help text: "−16 LUFS is a balanced default for spoken audio and music. The true peak limit helps prevent clipping after encoding."
  - Button: **Normalize Audio**.
- **Audio Visualizer:**
  - A preview.
  - Style: Waveform (the only option visible).
  - Shape: for example Landscape · 16:9.
  - Button: **Create Visualization**.
- **Convert Audio Channels:**
  - L and R waveform previews, each with ▶.
  - Output: **Mono** / **Stereo**.
  - Left volume and Right volume sliders (100%).
  - **▶ Play Output**.
  - Button: **Apply**.
- **Bleep Audio:**
  - A Bleep layer above the waveform. Drag across it to add a section, then drag a clip or either edge to adjust it.
  - A "Bleeps N" count.
  - Preview: **Original** / **▶ With Bleeps**.
  - Button: **Save Bleeped Copy**.

### PDF
- **Merge PDFs** (several files):
  - A "Document order" list with ▲/▼ buttons.
  - Button: **Merge PDFs**.
- **Organize PDF:**
  - An "N pages" count and a **Reset Order** button.
  - Page thumbnails with number badges and a rotate button on each. Drag a page to reorder.
  - A checkbox, "Make all pages the same width".
  - Button: **Save Organized PDF**.

## Output naming

Tangerine's cards show compressed outputs named `Compressed.jpg` and `Compressed.mov`. Quicky keeps the source name and adds a suffix instead (`name-small.jpg`, `name-trimmed.mp4`), so a batch never collides.

## Not possible on Linux

- Pages output.
- RTFD and text-clipping input.
- AirDrop: Quicky uses LocalSend in its place.
- EPUB output: needs Calibre, which isn't in a stock Omarchy install.
