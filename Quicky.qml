import Quickshell
import Quickshell.Io
import Quickshell.Hyprland
import Quickshell.Wayland
import QtQuick
import QtQuick.Shapes
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "Model.js" as Model

Item {
  id: root

  property var shell: null
  property var manifest: null

  readonly property string pluginId: (root.manifest && root.manifest.id) || "meshwa.oma-quicky"
  readonly property string backend: decodeURIComponent(String(Qt.resolvedUrl("bin/quicky")).replace(/^file:\/\//, ""))

  property bool opened: false
  property var files: []
  property var dragFiles: []
  property string mode: "convert"
  // Set once a key or scroll picks the wheel, so a drag arriving afterwards
  // doesn't put it back to the default.
  property bool modePicked: false
  property int selectedIndex: 0

  // Where the wheel opens, in the focused screen's logical coordinates.
  property var targetScreen: null
  property real cursorX: 0
  property real cursorY: 0

  // A drag in flight wins over loaded files so the wheel previews what a
  // drop would do.
  readonly property var activeFiles: dragFiles.length > 0 ? dragFiles : files
  readonly property var options: Model.optionsFor(mode, activeFiles)
  readonly property var selectedOption: selectedIndex >= 0 && selectedIndex < options.length ? options[selectedIndex] : null
  readonly property bool dropMode: activeFiles.length === 0

  // ------------------------------------------------------------ sound

  // `quicky sounds off` creates this flag; its presence mutes the wheel.
  readonly property string muteFlag: Quickshell.env("HOME") + "/.local/state/omarchy/toggles/quicky-muted"
  property bool muted: false
  readonly property real volume: 1.0

  FileView {
    path: root.muteFlag
    watchChanges: true
    printErrors: false
    onLoaded: root.muted = true
    onLoadFailed: root.muted = false
    onFileChanged: reload()
  }

  // Any tick.wav / shift.wav / run.wav in this folder replaces the bundled
  // sound, so people can bring their own without editing the plugin.
  readonly property string userSoundDir: Quickshell.env("HOME") + "/.config/omarchy/quicky/sounds"
  property var userSounds: []

  function soundUrl(name) {
    return root.userSounds.indexOf(name) >= 0
      ? "file://" + root.userSoundDir + "/" + name
      : Qt.resolvedUrl("sounds/" + name)
  }


  // Sounds play through pw-play in their own short-lived process rather than
  // QtMultimedia in the shell: an audio hiccup can then never stall the shell
  // that also runs the bar and lock screen. Each tick is its own process, so a
  // fast sweep overlaps naturally instead of cutting itself off.
  function soundPath(name) {
    return decodeURIComponent(String(root.soundUrl(name)).replace(/^file:\/\//, ""))
  }

  function play(name) {
    if (root.muted) return
    Quickshell.execDetached(["pw-play", "--latency", "10ms", "--volume", String(root.volume), root.soundPath(name)])
  }

  // Moving the highlight is the only thing that ticks — re-entering the same
  // segment, or the wheel resetting itself, stays quiet.
  function highlight(index) {
    if (index < 0 || index === root.selectedIndex) return
    root.selectedIndex = index
    root.play("tick.wav")
  }

  // Shares the [menu] surface tokens, like the emoji and clipboard pickers,
  // so any theme that styles the Omarchy menu styles Quicky too.
  property color background: Commons.Color.menu.background
  property color foreground: Commons.Color.menu.text
  property color border: Commons.Color.menu.border
  property color accent: Commons.Color.menu.selectedText
  property color urgent: Commons.Color.urgent
  property string fontFamily: Style.font.menuFamily

  // Polygon geometry. Always an octagon; slices don't have to line up with
  // sides.
  readonly property int sides: 8

  // Apothems (center to the middle of a side) for the outer ring edge, the
  // inner ring edge, and the hub. Past eight options the ring grows so labels
  // keep their room. The gap between segments follows the theme's window
  // gaps, so the ring tiles like windows.
  readonly property real outerApothem: Style.space(132) + Math.max(0, options.length - 8) * Style.space(14)
  readonly property real innerApothem: Style.space(62)
  readonly property real gap: Math.max(Style.space(3), Style.gapsOut)
  readonly property real hubApothem: innerApothem - gap
  readonly property real stroke: Math.max(1, Style.space(1))
  readonly property real ringSize: Math.ceil(outerApothem / Math.cos(Math.PI / sides)) * 2 + stroke * 4
  readonly property int captionHeight: Style.space(46)

  // ------------------------------------------------------------ lifecycle

  function open(payloadJson) {
    var payload = {}
    try { payload = JSON.parse(payloadJson || "{}") } catch (e) {}
    // Nautilus's selection can beat the drop summon here; keep the files.
    if (payload.drop === true && root.opened && root.files.length > 0) return
    root.files = Model.toPaths(payload.files || [])
    root.dragFiles = []
    // `drop: true` comes from a mid-drag summon: wait for the drop rather
    // than picking up whatever happens to be on the clipboard.
    probe.useClipboard = root.files.length === 0 && payload.drop !== true
    probe.running = true
  }

  function close() {
    root.opened = false
    root.dragFiles = []
  }

  function dismiss() {
    root.close()
    if (root.shell && typeof root.shell.hide === "function")
      root.shell.hide(root.pluginId)
  }

  function toggle() {
    if (root.opened) root.dismiss()
    else root.open("{}")
  }

  // Cursor, monitors, and (optionally) copied files arrive as one blob so
  // the wheel appears once, already in the right place.
  function applyProbe(raw) {
    var parts = String(raw || "").split("\n--quicky--\n")
    try {
      var cursor = JSON.parse(parts[0])
      var monitors = JSON.parse(parts[1])
      var mon = monitors.filter(function(m) { return m.focused })[0] || monitors[0]
      root.targetScreen = Quickshell.screens.filter(function(s) { return s.name === mon.name })[0] || Quickshell.screens[0]
      root.cursorX = cursor.x - mon.x
      root.cursorY = cursor.y - mon.y
    } catch (e) {
      root.targetScreen = Quickshell.screens[0]
      root.cursorX = root.targetScreen ? root.targetScreen.width / 2 : 0
      root.cursorY = root.targetScreen ? root.targetScreen.height / 2 : 0
    }
    // Only a real file list counts; stray copied text never becomes a target.
    if (probe.useClipboard && parts.length > 2)
      root.files = Model.toPaths(parts[2].split("\n"))
    // Re-listed on every open so a dropped-in sound applies without a restart.
    var sounds = parts.length > 3 ? parts[3].split("\n").filter(function(s) { return s.length > 0 }) : []
    if (sounds.join() !== root.userSounds.join()) root.userSounds = sounds
    root.mode = Model.defaultMode(root.files)
    root.modePicked = false
    root.selectedIndex = 0
    root.opened = true
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  Process {
    id: probe
    property bool useClipboard: true
    command: ["sh", "-c",
      "hyprctl -j cursorpos; printf '\\n--quicky--\\n'; hyprctl -j monitors; printf '\\n--quicky--\\n'; " +
      (useClipboard ? "wl-paste -n -t text/uri-list 2>/dev/null; " : "") +
      "printf '\\n--quicky--\\n'; cd \"$1\" 2>/dev/null && ls -1 tick.wav shift.wav run.wav 2>/dev/null",
      "sh", root.userSoundDir]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.applyProbe(text)
    }
  }

  // ------------------------------------------------------------ selection

  function setMode(next) {
    root.modePicked = true
    if (root.mode === next) return
    root.mode = next
    root.selectedIndex = 0
    root.play("shift.wav")
  }

  function cycleMode(delta) {
    var ids = Model.MODES.map(function(m) { return m.id })
    root.setMode(ids[(ids.indexOf(root.mode) + delta + ids.length) % ids.length])
  }

  function step(delta) {
    var n = root.options.length
    if (n > 0) root.highlight(root.selectedIndex < 0 ? 0 : (root.selectedIndex + delta + n) % n)
  }

  function activate(index) {
    if (index < 0 || index >= root.options.length || root.activeFiles.length === 0) return
    var argv = [root.backend, root.mode, root.options[index].id].concat(root.activeFiles)
    root.play("run.wav")
    root.dismiss()
    Quickshell.execDetached(argv)
  }

  // ------------------------------------------------------------ geometry

  // Distance from the center to the polygon's edge at angle `t`, for a
  // polygon with a flat side on top. Each side spans 360°/sides; within a
  // side the edge is a straight line, so the radius is apothem / cos(offset).
  function polyRadius(t, apothem) {
    var k = 2 * Math.PI / root.sides
    var u = t + Math.PI / 2
    return apothem / Math.cos(u - k * Math.round(u / k))
  }

  // Points along the polygon edge from angle `from` to `to`, including every
  // vertex in between, so the polygon keeps its sharp corners.
  function edgePoints(from, to, apothem, cx, cy) {
    var k = 2 * Math.PI / root.sides
    var angles = [from]
    var first = Math.ceil((from - (-Math.PI / 2 + k / 2)) / k)
    for (var j = first; ; j++) {
      var v = -Math.PI / 2 + k / 2 + j * k
      if (v >= to) break
      if (v > from) angles.push(v)
    }
    angles.push(to)
    return angles.map(function(t) {
      var r = root.polyRadius(t, apothem)
      return Qt.point(cx + Math.cos(t) * r, cy + Math.sin(t) * r)
    })
  }

  function sliceSpan() {
    return 2 * Math.PI / Math.max(1, root.options.length)
  }

  function sliceCenter(i) {
    return -Math.PI / 2 + i * root.sliceSpan()
  }

  // One ring segment: outer edge clockwise, inner edge back. The ends are
  // pulled in by half the gap at each radius so the gaps run parallel.
  function segmentPoints(i, cx, cy) {
    var span = root.sliceSpan()
    var a0 = root.sliceCenter(i) - span / 2
    var a1 = a0 + span
    var half = root.gap / 2
    function trim(apothem) {
      var r = root.polyRadius(a0, apothem)
      return Math.asin(Math.min(1, half / r))
    }
    var outer = root.edgePoints(a0 + trim(root.outerApothem), a1 - trim(root.outerApothem), root.outerApothem, cx, cy)
    var inner = root.edgePoints(a0 + trim(root.innerApothem), a1 - trim(root.innerApothem), root.innerApothem, cx, cy)
    var pts = outer.concat(inner.reverse())
    pts.push(pts[0])
    return pts
  }

  function polygonPoints(apothem, cx, cy) {
    var pts = root.edgePoints(-Math.PI, Math.PI, apothem, cx, cy)
    pts.push(pts[0])
    return pts
  }

  // Whether a point is on the octagon (hub or segments) rather than the
  // empty corners of the panel around it.
  function insideRing(x, y) {
    var dx = x - ring.width / 2
    var dy = y - ring.height / 2
    return Math.sqrt(dx * dx + dy * dy) <= root.polyRadius(Math.atan2(dy, dx), root.outerApothem)
  }

  // Pie-menu hit test: anywhere in the ring's direction counts, so the
  // pointer never has to land exactly on a label.
  function indexAt(x, y) {
    var dx = x - ring.width / 2
    var dy = y - ring.height / 2
    var t = Math.atan2(dy, dx)
    var d = Math.sqrt(dx * dx + dy * dy)
    if (root.options.length === 0 || d < root.polyRadius(t, root.hubApothem)) return -1
    var span = root.sliceSpan()
    var u = t + Math.PI / 2 + span / 2
    return Math.floor(((u % (2 * Math.PI)) + 2 * Math.PI) % (2 * Math.PI) / span) % root.options.length
  }

  // ------------------------------------------------------------ surface

  // The surface covers the whole screen, so a press anywhere off the wheel
  // lands on it and closes the wheel. (A separate catcher layer, and
  // Hyprland's focus grab, both let such presses through to the windows.)
  // To drop files, start the drag first and summon the wheel mid-drag.
  PanelWindow {
    id: panel
    visible: root.opened
    screen: root.targetScreen
    // Not fully transparent, so no compositor treats it as click-through.
    color: Qt.rgba(0, 0, 0, 1 / 255)
    anchors { top: true; bottom: true; left: true; right: true }
    WlrLayershell.namespace: "omarchy-quicky"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive
    exclusionMode: ExclusionMode.Ignore

    MouseArea {
      anchors.fill: parent
      acceptedButtons: Qt.AllButtons
      onPressed: root.dismiss()
    }

    Item {
      id: content
      x: Math.round(Util.clamp(root.cursorX - root.ringSize / 2, 0, Math.max(0, panel.width - root.ringSize)))
      y: Math.round(Util.clamp(root.cursorY - root.ringSize / 2, 0, Math.max(0, panel.height - root.ringSize - root.captionHeight)))
      width: root.ringSize
      height: root.ringSize + root.captionHeight
      opacity: root.opened ? 1 : 0
      scale: root.opened ? 1 : 0.96
      Behavior on opacity { NumberAnimation { duration: 90 } }
      Behavior on scale { NumberAnimation { duration: 110; easing.type: Easing.OutCubic } }

      Item {
        id: keyCatcher
        anchors.fill: parent
        focus: true

        Keys.priority: Keys.BeforeItem
        Keys.onPressed: function(event) {
          var k = event.key
          if (k === Qt.Key_Escape) root.dismiss()
          else if (k === Qt.Key_Right) root.step(1)
          else if (k === Qt.Key_Left) root.step(-1)
          // One tap switches wheels, like Tangerine: Shift converts, Alt
          // (where ⌘ sits on a Mac keyboard) opens tools, Ctrl actions.
          else if (k === Qt.Key_Shift) root.setMode("convert")
          else if (k === Qt.Key_Alt) root.setMode("tool")
          else if (k === Qt.Key_Control) root.setMode("action")
          else if (k === Qt.Key_Tab || k === Qt.Key_Down) root.cycleMode(1)
          else if (k === Qt.Key_Up) root.cycleMode(-1)
          else if (k === Qt.Key_Return || k === Qt.Key_Enter) root.activate(root.selectedIndex)
          else if (k >= Qt.Key_1 && k <= Qt.Key_9) root.activate(k - Qt.Key_1)
          else return
          event.accepted = true
        }
      }

      Item {
        id: ring
        width: root.ringSize
        height: root.ringSize

        // Empty ring while waiting for a drop, so the target is obvious.
        Shape {
          anchors.fill: parent
          visible: root.options.length === 0
          preferredRendererType: Shape.CurveRenderer
          ShapePath {
            fillColor: Util.alpha(root.background, dropArea.containsDrag ? 0.96 : 0.88)
            strokeColor: dropArea.containsDrag ? root.accent : Util.alpha(root.foreground, 0.3)
            strokeWidth: root.stroke
            fillRule: ShapePath.OddEvenFill
            PathPolyline { path: root.polygonPoints(root.outerApothem, ring.width / 2, ring.height / 2) }
            PathPolyline { path: root.polygonPoints(root.innerApothem, ring.width / 2, ring.height / 2) }
          }
        }

        Repeater {
          model: root.options

          delegate: Item {
            id: slice
            required property var modelData
            required property int index

            readonly property bool hot: index === root.selectedIndex
            readonly property color ink: modelData.danger ? root.urgent : (hot ? root.accent : root.foreground)
            readonly property real mid: root.sliceCenter(index)
            readonly property real labelRadius: (root.polyRadius(mid, root.outerApothem) + root.polyRadius(mid, root.innerApothem)) / 2

            anchors.fill: parent

            Shape {
              anchors.fill: parent
              preferredRendererType: Shape.CurveRenderer
              ShapePath {
                fillColor: slice.hot ? Qt.tint(root.background, Util.alpha(slice.ink, 0.16)) : Util.alpha(root.background, 0.94)
                strokeColor: slice.hot ? slice.ink : Util.alpha(root.foreground, 0.22)
                strokeWidth: slice.hot ? root.stroke * 2 : root.stroke
                joinStyle: ShapePath.MiterJoin
                PathPolyline { path: root.segmentPoints(slice.index, ring.width / 2, ring.height / 2) }
              }
            }

            Column {
              x: ring.width / 2 + Math.cos(slice.mid) * slice.labelRadius - width / 2
              y: ring.height / 2 + Math.sin(slice.mid) * slice.labelRadius - height / 2
              width: Style.space(78)
              spacing: Style.spacing.xxs

              Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: slice.modelData.glyph
                color: slice.ink
                font.family: root.fontFamily
                font.pixelSize: Style.font.iconLarge
              }
              Text {
                width: parent.width
                horizontalAlignment: Text.AlignHCenter
                textFormat: Text.PlainText
                text: slice.modelData.label
                color: slice.ink
                elide: Text.ElideRight
                font.family: root.fontFamily
                font.pixelSize: Style.font.bodySmall
                font.bold: slice.hot
              }
            }
          }
        }

        // Hub: the loaded file.
        Shape {
          anchors.fill: parent
          preferredRendererType: Shape.CurveRenderer
          ShapePath {
            fillColor: root.background
            strokeColor: root.border
            strokeWidth: root.stroke
            joinStyle: ShapePath.MiterJoin
            PathPolyline { path: root.polygonPoints(root.hubApothem, ring.width / 2, ring.height / 2) }
          }
        }

        Column {
          anchors.centerIn: parent
          width: root.hubApothem * 1.7
          spacing: Style.spacing.xs

          Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: root.dropMode ? "󰉍" : Model.glyphFor(root.activeFiles)
            color: root.accent
            font.family: root.fontFamily
            font.pixelSize: Style.font.display
          }
          Text {
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            textFormat: Text.PlainText
            text: root.dropMode ? "Drop files" : Model.summary(root.activeFiles)
            color: root.foreground
            elide: Text.ElideMiddle
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
          }
        }

        MouseArea {
          anchors.fill: parent
          hoverEnabled: true
          acceptedButtons: Qt.LeftButton | Qt.RightButton
          onPositionChanged: function(mouse) {
            root.highlight(root.indexAt(mouse.x, mouse.y))
          }
          onClicked: function(mouse) {
            // The panel is a square: its corners and the caption are
            // "anywhere else" too.
            if (mouse.button === Qt.RightButton || !root.insideRing(mouse.x, mouse.y)) root.dismiss()
            else root.activate(root.indexAt(mouse.x, mouse.y))
          }
          onWheel: function(wheel) { root.cycleMode(wheel.angleDelta.y > 0 ? -1 : 1) }
        }

        // Drag files in from anywhere. Dropping on a segment runs it straight
        // away; dropping on the hub just loads the files.
        DropArea {
          id: dropArea
          anchors.fill: parent
          onEntered: function(drag) {
            root.dragFiles = Model.toPaths(drag.urls)
            if (!root.modePicked) root.mode = Model.defaultMode(root.dragFiles)
            root.selectedIndex = -1
          }
          onPositionChanged: function(drag) {
            var i = root.indexAt(drag.x, drag.y)
            if (i < 0) root.selectedIndex = -1
            else root.highlight(i)
          }
          onExited: root.dragFiles = []
          onDropped: function(drop) {
            var i = root.indexAt(drop.x, drop.y)
            drop.acceptProposedAction()
            root.files = Model.toPaths(drop.urls)
            root.dragFiles = []
            if (i >= 0) root.activate(i)
            else root.selectedIndex = 0
          }
        }
      }

      // Tooltip: what the pick will produce, plus which key opens which wheel.
      Rectangle {
        id: tip
        anchors.top: ring.bottom
        anchors.topMargin: Style.spacing.md
        anchors.horizontalCenter: ring.horizontalCenter
        visible: !root.dropMode
        // Sized from the text's natural width; binding to the Column's own
        // width would loop and collapse the tooltip to nothing.
        width: Math.min(root.ringSize, Math.max(tipLine.implicitWidth, tipKeys.implicitWidth) + Style.spacing.xl * 2)
        height: tipColumn.implicitHeight + Style.spacing.md * 2
        color: Commons.Color.tooltip.background
        border.width: root.stroke
        border.color: Commons.Color.tooltip.border

        Column {
          id: tipColumn
          anchors.centerIn: parent
          width: parent.width - Style.spacing.xl * 2
          spacing: Style.spacing.xxs

          Text {
            id: tipLine
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            textFormat: Text.PlainText
            text: Model.preview(root.mode, root.selectedOption, root.activeFiles)
            color: Commons.Color.tooltip.text
            elide: Text.ElideMiddle
            font.family: root.fontFamily
            font.pixelSize: Style.font.bodySmall
          }
          Text {
            id: tipKeys
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            textFormat: Text.StyledText
            // Key symbols only; the active wheel also shows its name.
            text: Model.MODES.map(function(m) {
              return m.id === root.mode ? "<b>" + m.key + " " + m.label + "</b>" : m.key
            }).join("   ")
            color: Commons.Color.tooltip.text
            opacity: 0.55
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
          }
        }
      }
    }
  }
}
