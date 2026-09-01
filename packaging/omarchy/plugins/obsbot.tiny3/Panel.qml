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
  moduleName: "obsbot.tiny3"
  ipcTarget: "obsbot.tiny3"

  // Cheap, non-invasive (sysfs) — drives the bar icon dimming.
  property string powerState: "unknown" // "active" | "suspended" | "unknown"
  property bool present: true

  // Full state, refreshed on open + after actions (control read; no wake).
  property bool asleep: false
  property string tracking: "off"
  property bool autoWb: true
  property int wbTemp: 4000
  property bool hdr: false

  readonly property color fg: bar ? bar.foreground : Color.foreground
  readonly property real rowW: Style.space(300)

  // --- data plumbing ---

  function refreshPower() { if (!powerProc.running) powerProc.running = true }
  function refreshStatus() { if (!statusProc.running) statusProc.running = true }

  function act(args) {
    Quickshell.execDetached(["t3ctl"].concat(args))
    afterAction.restart()
  }

  function preview() { Quickshell.execDetached(["t3-preview"]) }

  // Panel base's open() — refresh full state as the popup appears.
  function open() {
    refreshStatus()
    root.controller.show()
  }

  Component.onCompleted: refreshPower()

  Timer { interval: 8000; running: true; repeat: true; onTriggered: root.refreshPower() }
  Timer { id: afterAction; interval: 500; onTriggered: { root.refreshStatus(); root.refreshPower() } }

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
          root.asleep = j.asleep === true
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
    // Nerd Font camera glyph; dimmed when the camera is idle/suspended.
    text: ""
    opacity: (root.powerState === "active") ? 1.0 : 0.55
    tooltipText: "OBSBOT Tiny 3"
    onPressed: function(b) {
      if (b === Qt.MiddleButton) { root.preview(); return }
      if (b === Qt.RightButton) { root.act(["toggle"]); return }
      root.toggle()
    }
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

        PanelSectionHeader {
          text: "OBSBOT TINY 3"
          foreground: root.fg
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

        Button {
          width: parent.width
          leftAlign: true
          foreground: root.fg
          text: root.asleep ? "Wake camera" : "Sleep camera"
          onClicked: root.act(["toggle"])
        }
        Button {
          width: parent.width
          leftAlign: true
          foreground: root.fg
          text: (root.tracking !== "off") ? "AI tracking: on" : "AI tracking: off"
          onClicked: root.act(["track", "toggle"])
        }
        Button {
          width: parent.width
          leftAlign: true
          foreground: root.fg
          text: "White balance: " + (root.autoWb ? "auto" : "manual " + root.wbTemp + "K")
          onClicked: root.act(["wb", root.autoWb ? "pin" : "auto"])
        }
        Button {
          width: parent.width
          leftAlign: true
          foreground: root.fg
          text: root.hdr ? "HDR: on" : "HDR: off"
          onClicked: root.act(["hdr", root.hdr ? "off" : "on"])
        }
        Button {
          width: parent.width
          leftAlign: true
          foreground: root.fg
          text: "Recenter gimbal"
          onClicked: root.act(["recenter"])
        }

        PanelSeparator { foreground: root.fg }

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
