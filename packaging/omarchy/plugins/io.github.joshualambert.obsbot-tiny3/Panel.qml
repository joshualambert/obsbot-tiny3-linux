import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

// OBSBOT Tiny 3 bar widget: a camera icon in the bar that opens a control popup
// (sleep/wake, AI tracking, white balance, HDR, recenter) plus a live preview.
//
// Design note on the camera-sleep goal: the bar icon reflects only the cheap,
// non-invasive USB power state (t3ctl power reads sysfs, opens nothing). The
// full status (t3ctl status) is read on popup-open and after each action — a
// control read that does not wake a sleeping camera. The live preview STREAMS
// the camera (which wakes it) and runs in its own window, so it is on-demand
// and releases the camera the moment it is closed.
Panel {
  id: root
  moduleName: "io.github.joshualambert.obsbot-tiny3"
  ipcTarget: "io.github.joshualambert.obsbot-tiny3"
  manageIpc: false

  // Cheap, non-invasive (sysfs) — drives the bar icon dimming.
  property string powerState: "unknown" // "active" | "suspended" | "unknown"
  property bool present: true

  // Full state, refreshed on open + after actions (control read; no wake).
  property bool asleep: false
  property string tracking: "off"
  property bool autoWb: true
  property int wbTemp: 4000
  property bool hdr: false

  // Preview capture resolution, chosen in the popup dropdown.
  property string previewRes: "1920x1080"

  readonly property color fg: bar ? bar.foreground : Color.foreground
  readonly property real rowW: Style.space(300)

  // Whimsy: a little rotating tagline under the header, and a live pulse.
  readonly property var phrases: [
    "Watching the room",
    "Framing you up",
    "Lights, camera…",
    "Eyes on you",
    "Keeping you centered",
    "Ready for your close-up",
    "Rolling",
    "Looking sharp"
  ]
  property int phraseIndex: 0
  property real pulse: 1.0

  // A control row where ONLY the switch is clickable (unlike the stock Toggle,
  // which makes the whole row a hit target). Label + optional sub-label on the
  // left, a ToggleSwitch on the right.
  component SwitchRow: Item {
    id: sr
    property string label: ""
    property string sub: ""
    property bool value: false
    property color fg: "white"
    property string fam: Style.font.family
    signal switched()
    width: parent ? parent.width : Style.space(300)
    implicitHeight: Math.max(labelCol.implicitHeight, sw.implicitHeight) + Style.space(12)
    Column {
      id: labelCol
      anchors.left: parent.left
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(2)
      Text {
        text: sr.label
        color: sr.fg
        font.family: sr.fam
        font.pixelSize: Style.font.subtitle
      }
      Text {
        text: sr.sub
        visible: sr.sub !== ""
        color: Qt.darker(sr.fg, 1.4)
        font.family: sr.fam
        font.pixelSize: Style.font.caption
      }
    }
    ToggleSwitch {
      id: sw
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      checked: sr.value
      foreground: sr.fg
      onToggled: sr.switched()
    }
  }

  // Report the bar-slot size from the icon button, or the bar collapses this
  // widget to zero width and nothing shows (mirrors omarchy.power).
  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  // --- data plumbing ---

  function refreshPower() { if (!powerProc.running) powerProc.running = true }
  function refreshStatus() { if (!statusProc.running) statusProc.running = true }

  function act(args) {
    Quickshell.execDetached(["t3ctl"].concat(args))
    afterAction.restart()
  }

  function preview() { Quickshell.execDetached(["t3-preview", root.previewRes]) }

  // Panel base's open() — refresh full state as the popup appears.
  function open() {
    refreshStatus()
    root.controller.show()
  }

  Component.onCompleted: refreshPower()

  Timer { interval: 8000; running: true; repeat: true; onTriggered: root.refreshPower() }
  Timer { id: afterAction; interval: 700; onTriggered: { root.refreshStatus(); root.refreshPower() } }
  // Rotate the tagline only while the popup is open.
  Timer {
    interval: 3200
    running: root.opened
    repeat: true
    onTriggered: root.phraseIndex = (root.phraseIndex + 1) % root.phrases.length
  }

  Process {
    id: powerProc
    command: ["t3ctl", "power", "--json"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        try {
          var j = JSON.parse(text)
          root.powerState = String(j.usb_power || "unknown")
          root.present = true
        } catch (e) {
          root.present = false
        }
      }
    }
  }

  Process {
    id: statusProc
    command: ["t3ctl", "status", "--json"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        try {
          var j = JSON.parse(text)
          // NB: we deliberately do NOT reconcile `asleep` from the readback.
          // Reading the sleep bit means opening the device, which disturbs the
          // very state we're reading, so it's unreliable — the Awake switch is
          // driven optimistically from the user's action instead. The other
          // fields are reliable (unaffected by the read).
          root.tracking = String(j.tracking || "off")
          root.autoWb = j.auto_wb === true
          root.wbTemp = j.wb_temp | 0
          root.hdr = j.hdr === true
          root.present = true
        } catch (e) {
          // leave last-known values
        }
      }
    }
  }

  // --- bar icon ---

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    // Nerd Font webcam glyph.
    text: "󰖠"
    // Full opacity whenever the camera is present; faint only when unplugged.
    opacity: root.present ? 1.0 : 0.4
    tooltipText: "OBSBOT Tiny 3"
    // Gentle "live" pulse while the camera is in use (USB active).
    scale: (root.powerState === "active") ? root.pulse : 1.0
    Behavior on opacity { NumberAnimation { duration: 220 } }
    Behavior on scale { NumberAnimation { duration: 180; easing.type: Easing.OutBack } }
    onPressed: function(b) {
      if (b === Qt.MiddleButton) { root.preview(); return }
      if (b === Qt.RightButton) { root.act(["toggle"]); return }
      root.toggle()
    }
  }

  // The heartbeat that drives the icon's live pulse.
  SequentialAnimation on pulse {
    running: root.powerState === "active"
    loops: Animation.Infinite
    NumberAnimation { to: 1.14; duration: 720; easing.type: Easing.InOutSine }
    NumberAnimation { to: 1.0; duration: 720; easing.type: Easing.InOutSine }
  }

  // --- popup ---

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(root.rowW)
    contentHeight: panel.fittedContentHeight(column.implicitHeight)

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }

      Column {
        id: column
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        spacing: Style.space(6)

        // Entrance: fade in and rise with a little overshoot each time the
        // popup opens.
        opacity: 0
        transform: Translate { id: slideT; y: 16 }
        Connections {
          target: root
          function onOpenedChanged() {
            if (root.opened) revealAnim.restart()
          }
        }
        Component.onCompleted: if (root.opened) { column.opacity = 1; slideT.y = 0 }
        ParallelAnimation {
          id: revealAnim
          NumberAnimation { target: column; property: "opacity"; from: 0; to: 1; duration: 240; easing.type: Easing.OutCubic }
          NumberAnimation { target: slideT; property: "y"; from: 16; to: 0; duration: 340; easing.type: Easing.OutBack }
        }

        PanelSectionHeader {
          text: "OBSBOT TINY 3"
          foreground: root.fg
        }

        // Whimsical rotating tagline (soft crossfade on change).
        Text {
          id: tagline
          width: parent.width
          color: Color.accent
          font.family: root.bar ? root.bar.fontFamily : Style.font.family
          font.pixelSize: Style.font.caption
          font.italic: true
          elide: Text.ElideRight
          text: root.present ? root.phrases[root.phraseIndex] : "Say hi to the camera"
          opacity: 0.85
          Behavior on opacity { NumberAnimation { duration: 260; easing.type: Easing.InOutSine } }
          Connections {
            target: root
            function onPhraseIndexChanged() { tagline.opacity = 0; taglineIn.restart() }
          }
          Timer { id: taglineIn; interval: 180; onTriggered: tagline.opacity = 0.85 }
        }

        Text {
          width: parent.width
          color: Qt.darker(root.fg, 1.2)
          font.family: root.bar ? root.bar.fontFamily : Style.font.family
          font.pixelSize: Style.font.caption
          wrapMode: Text.WordWrap
          text: root.present
            ? ((root.asleep ? "Asleep" : "Awake")
               + " · tracking " + root.tracking
               + " · WB " + (root.autoWb ? "auto" : (root.wbTemp + "K"))
               + (root.hdr ? " · HDR" : ""))
            : "Camera not found"
        }

        PanelSeparator { foreground: root.fg }

        // Optimistic switches: flip the local state on click for instant
        // feedback, then send the command. Sleep uses explicit sleep/wake
        // (not toggle) so its direction never depends on an unreliable readback.
        SwitchRow {
          width: parent.width
          label: "Awake"
          sub: root.asleep ? "camera asleep" : ""
          value: !root.asleep
          fg: root.fg
          fam: root.bar ? root.bar.fontFamily : Style.font.family
          onSwitched: {
            if (root.asleep) { root.asleep = false; root.act(["wake"]) }
            else { root.asleep = true; root.act(["sleep"]) }
          }
        }
        SwitchRow {
          width: parent.width
          label: "AI tracking"
          value: root.tracking !== "off"
          fg: root.fg
          fam: root.bar ? root.bar.fontFamily : Style.font.family
          onSwitched: {
            var on = root.tracking === "off"
            root.tracking = on ? "normal" : "off"
            root.act(["track", on ? "on" : "off"])
          }
        }
        SwitchRow {
          width: parent.width
          label: "HDR"
          value: root.hdr
          fg: root.fg
          fam: root.bar ? root.bar.fontFamily : Style.font.family
          onSwitched: {
            root.hdr = !root.hdr
            root.act(["hdr", root.hdr ? "on" : "off"])
          }
        }
        SwitchRow {
          width: parent.width
          label: "Auto white balance"
          sub: root.autoWb ? "" : ("pinned " + root.wbTemp + "K")
          value: root.autoWb
          fg: root.fg
          fam: root.bar ? root.bar.fontFamily : Style.font.family
          onSwitched: {
            root.autoWb = !root.autoWb
            root.act(["wb", root.autoWb ? "auto" : "pin"])
          }
        }

        PanelSeparator { foreground: root.fg }

        Dropdown {
          width: parent.width
          label: "Preview resolution"
          value: root.previewRes
          options: [
            { value: "1280x720", label: "720p" },
            { value: "1920x1080", label: "1080p" },
            { value: "3840x2160", label: "2160p (4K)" }
          ]
          onChanged: function(v) { root.previewRes = v }
        }

        Button {
          width: parent.width
          leftAlign: true
          foreground: root.fg
          text: "Recenter gimbal"
          onClicked: root.act(["recenter"])
        }
        Button {
          width: parent.width
          leftAlign: true
          foreground: root.fg
          text: "Live preview"
          onClicked: root.preview()
        }
      }
    }
  }
}
