.pragma library

// What each wheel offers for a given set of files. The ids here are the
// arguments `bin/quicky` understands, so this file and the backend must agree.
// The per-kind lists follow Tangerine's (see docs/tangerine-features.md).

var MODES = [
  { id: "convert", label: "Convert", key: "⇧" },
  { id: "tool", label: "Tools", key: "⌥" },
  { id: "action", label: "Actions", key: "⌃" }
]

var KINDS = {
  image: ["jpg", "jpeg", "png", "webp", "heic", "heif", "tif", "tiff", "avif", "bmp", "svg", "ico"],
  gif: ["gif"],
  video: ["mp4", "mov", "mkv", "webm", "avi", "wmv", "m4v", "flv"],
  audio: ["mp3", "m4a", "wav", "flac", "ogg", "opus", "aiff", "aif", "wma", "aac"],
  pdf: ["pdf"],
  text: ["docx", "doc", "odt", "rtf", "txt", "html", "htm", "md", "markdown"],
  slides: ["pptx", "ppt", "odp"],
  sheet: ["xlsx", "xls", "ods", "csv", "tsv"],
  subtitle: ["srt", "vtt"],
  archive: ["zip", "7z", "tar", "gz", "tgz", "bz2", "xz", "rar"]
}

var CONVERT = {
  image: ["png", "jpg", "webp", "heic", "avif", "tiff", "bmp", "pdf"],
  gif: ["png", "mp4", "webm"],
  video: ["mp4", "mov", "mkv", "webm", "avi", "wmv", "gif", "mp3"],
  audio: ["mp3", "m4a", "wav", "flac", "ogg", "opus", "aiff", "wma"],
  pdf: ["docx", "md", "txt", "pptx", "png", "jpg"],
  text: ["pdf", "docx", "md", "txt", "html", "odt", "png", "jpg"],
  slides: ["pdf", "pptx", "odp", "png", "jpg"],
  sheet: ["pdf", "xlsx", "ods", "csv", "tsv"],
  subtitle: ["srt", "vtt", "txt"],
  archive: ["zip", "7z", "tar", "gz"]
}

var GLYPHS = {
  image: "󰋩",
  gif: "󰵸",
  video: "󰕧",
  audio: "󰎆",
  pdf: "󰈦",
  text: "󰈙",
  slides: "󰈧",
  sheet: "󰈛",
  subtitle: "󰨖",
  archive: "󰗄"
}

// Every tool, keyed by the id `bin/quicky` takes. A tool marked
// `ready: false` stays off the wheel (for one whose editor isn't built).
var TOOLS = {
  compress: { label: "Compress", glyph: "󰘕", hint: "Smaller copy, original kept" },
  strip: { label: "Metadata", glyph: "", hint: "Inspect, edit, or remove metadata" },
  edit: { label: "Edit", glyph: "󰘮", hint: "Adjust light, color, and detail" },
  annotate: { label: "Annotate", glyph: "󰏫", hint: "Arrows, text, shapes, callouts" },
  addbg: { label: "Add BG", glyph: "󰹉", hint: "Place on a new background" },
  crop: { label: "Crop", glyph: "󰆞", hint: "Open the crop editor" },
  redact: { label: "Redact", glyph: "󰈉", hint: "Black out, pixelate, or blur areas" },
  mute: { label: "Mute", glyph: "󰝟", suffix: "-muted" },
  trim: { label: "Trim", glyph: "󰆐", hint: "Keep one section" },
  "crop-video": { label: "Crop", glyph: "󰆞", hint: "Reframe the video" },
  speed: { label: "Speed", glyph: "󰓅", hint: "Slow down or speed up" },
  snapshot: { label: "Snapshot", glyph: "󰄀", hint: "Save chosen frames as PNGs" },
  "split-video": { label: "Split", glyph: "󰃺", hint: "Cut into clips" },
  "redact-video": { label: "Redact", glyph: "󰈉", hint: "Hide areas over time" },
  normalize: { label: "Normalize", glyph: "󰕾", hint: "Even out the loudness" },
  visualize: { label: "Visualize", glyph: "󰺢", hint: "Waveform or still-image MP4" },
  channels: { label: "Channels", glyph: "󰓃", hint: "Mono or stereo, balance each side" },
  bleep: { label: "Bleep", glyph: "󰐰", hint: "Beep over sections" },
  split: { label: "Split", glyph: "󰃺", hint: "One PDF per page" },
  organize: { label: "Organize", glyph: "󰕰", hint: "Reorder and rotate pages" },
  extract: { label: "Extract", glyph: "󰛫", hint: "Unpack into a folder" },
  join: { label: "Join", glyph: "󰕘", hint: "Arrange the order, save one file" },
  merge: { label: "Merge", glyph: "󰕘", hint: "Arrange the order, save one PDF" },
  "make-pdf": { label: "Create PDF", glyph: "󰈦", hint: "One page per image" },
  collage: { label: "Collage", glyph: "󰕰", hint: "Grid, row, column, or featured" }
}

// Each kind's Tools wheel, clockwise from the top. One file:
var SINGLE = {
  image: ["compress", "strip", "edit", "annotate", "addbg", "crop", "redact"],
  video: ["compress", "strip", "mute", "trim", "crop-video", "speed", "snapshot", "split-video", "redact-video"],
  audio: ["compress", "strip", "normalize", "visualize", "trim", "channels", "bleep"],
  pdf: ["compress", "strip", "split", "organize"],
  gif: ["compress", "strip"],
  archive: ["extract"],
  "*": ["strip"]
}

// Several files, all the same kind. A mix of kinds gets no tools.
var MANY = {
  image: ["compress", "make-pdf", "collage"],
  video: ["compress", "join"],
  audio: ["compress", "join"],
  pdf: ["compress", "merge"]
}

var ACTIONS = [
  { id: "send", label: "Send", glyph: "󰒖", hint: "Send with LocalSend" },
  { id: "float", label: "Float", glyph: "󰐃", hint: "Open in a small pinned window" },
  { id: "copy-file", label: "Copy", glyph: "󰆏", hint: "Copy the file, paste it in Files" },
  { id: "move", label: "Move", glyph: "󰉒", hint: "Move to another folder" },
  { id: "copy-path", label: "Path", glyph: "󰅍", hint: "Copy the path to the clipboard" },
  { id: "terminal", label: "Terminal", glyph: "", hint: "Open a terminal here" },
  { id: "zip", label: "Zip", glyph: "󰗄", hint: "Pack into a .zip" },
  { id: "trash", label: "Trash", glyph: "󰩹", hint: "Move to trash", danger: true }
]

function extOf(path) {
  var base = String(path).split("/").pop()
  var dot = base.lastIndexOf(".")
  return dot > 0 ? base.substr(dot + 1).toLowerCase() : ""
}

function nameOf(path) {
  return String(path).replace(/\/+$/, "").split("/").pop()
}

function kindOf(path) {
  var ext = extOf(path)
  for (var kind in KINDS) {
    if (KINDS[kind].indexOf(ext) >= 0) return kind
  }
  return "other"
}

// Accepts file:// URLs (drops, uri-lists) and plain paths; returns paths.
function toPaths(list) {
  var out = []
  for (var i = 0; i < list.length; i++) {
    var s = String(list[i]).replace(/^\s+|\s+$/g, "")
    if (!s || s.charAt(0) === "#") continue
    if (s.indexOf("file://") === 0) s = decodeURIComponent(s.substr(7))
    if (s.charAt(0) === "/") out.push(s)
  }
  return out
}

function intersect(lists) {
  if (lists.length === 0) return []
  return lists[0].filter(function(id) {
    return lists.every(function(l) { return l.indexOf(id) >= 0 })
  })
}

function convertOptions(paths) {
  var lists = paths.map(function(p) {
    var targets = CONVERT[kindOf(p)] || []
    var ext = extOf(p) === "jpeg" ? "jpg" : extOf(p)
    return targets.filter(function(t) { return t !== ext })
  })
  return intersect(lists).map(function(t) {
    return { id: t, label: t === "gz" ? "TAR.GZ" : t.toUpperCase(), glyph: GLYPHS[kindOf("x." + t)] || "󰈔" }
  })
}

function toolOptions(paths) {
  var kind = kindOf(paths[0])
  var ids
  if (paths.length === 1) {
    ids = SINGLE[kind] || SINGLE["*"]
  } else {
    var same = paths.every(function(p) { return kindOf(p) === kind })
    ids = same ? (MANY[kind] || []) : []
  }
  return ids.filter(function(id) { return TOOLS[id].ready !== false }).map(function(id) {
    return { id: id, label: TOOLS[id].label, glyph: TOOLS[id].glyph }
  })
}

function optionsFor(mode, paths) {
  if (!paths || paths.length === 0) return []
  if (mode === "convert") return convertOptions(paths)
  if (mode === "tool") return toolOptions(paths)
  return ACTIONS
}

// The mode to land on when the wheel opens: convert if anything converts.
function defaultMode(paths) {
  return convertOptions(paths).length > 0 ? "convert" : (toolOptions(paths).length > 0 ? "tool" : "action")
}

// The caption under the wheel: what picking this option will produce.
function preview(mode, option, paths) {
  if (!option || paths.length === 0) return ""
  var stem = nameOf(paths[0]).replace(/\.[^.]*$/, "")
  var more = paths.length > 1 ? "  +" + (paths.length - 1) + " more" : ""
  if (mode === "convert") return "→ " + stem + "." + (option.id === "gz" ? "tar.gz" : option.id) + more
  var tool = mode === "tool" ? TOOLS[option.id] : null
  if (tool && tool.suffix)
    return "→ " + stem + tool.suffix + "." + (tool.ext || extOf(paths[0])) + more
  if (mode === "tool") return TOOLS[option.id].hint
  return option.hint || option.label
}

function glyphFor(paths) {
  if (paths.length !== 1) return "󰉋"
  return GLYPHS[kindOf(paths[0])] || "󰈔"
}

function summary(paths) {
  if (paths.length === 0) return "Drop files"
  if (paths.length === 1) return nameOf(paths[0])
  return paths.length + " files"
}
