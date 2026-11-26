import QtQuick
import QtQuick.Layouts
import QtQuick.Shapes
import org.kde.plasma.plasmoid
import org.kde.plasma.plasma5support as P5Support
import org.kde.plasma.private.battery

// The rail's bottom end (Bento mock 02), stacked and centred in the 68 px rail: the Wi-Fi and
// volume glyphs (2 px line icons, 16 px, muted), the battery percentage (11 px bold) and the
// time as two lines (15 px, weight 800). Wi-Fi shows while connected over Wi-Fi (a plug over a
// cable), the volume glyph shows muted when the sound is off, the percentage only on a computer
// with a battery. Nothing is invented: a part with nothing to say is left out. A press opens the
// one quick panel (tripppin-ui quick).
PlasmoidItem {
    id: root
    preferredRepresentation: fullRepresentation

    property string net: ""
    property bool muted: false
    property date now: new Date()

    // The glyph paths are the design's (Bento mock section 08), on a 24 unit grid.
    readonly property string wifiPath: "M2 9a15 15 0 0 1 20 0M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0M12 19.5h.01"
    readonly property string plugPath: "M3 5h18v10H3zM8 19h8M12 15v4"
    readonly property string volumePath: "M4 9h4l5-4v14l-5-4H4zM16 9a4 4 0 0 1 0 6M19 6a8 8 0 0 1 0 12"
    readonly property string mutedPath: "M4 9h4l5-4v14l-5-4H4zM17 9l5 6M22 9l-5 6"

    BatteryControlModel { id: battery }

    // Writes the unread counts the lock screen shows (its own file, so a failure here only
    // loses the badges).
    Loader { source: "Unread.qml" }

    Timer {
        interval: 1000
        running: true
        repeat: true
        onTriggered: root.now = new Date()
    }

    P5Support.DataSource {
        id: nm
        engine: "executable"
        connectedSources: ["nmcli -t -f TYPE,STATE device", "wpctl get-volume @DEFAULT_AUDIO_SINK@"]
        interval: 5000
        onNewData: (source, data) => {
            var out = data["stdout"] || "";
            if (source.indexOf("wpctl") === 0) {
                root.muted = out.indexOf("[MUTED]") >= 0;
                return;
            }
            if (out.indexOf("wifi:connected\n") >= 0 || /wifi:connected$/m.test(out))
                root.net = "Wi-Fi";
            else if (out.indexOf("ethernet:connected") >= 0)
                root.net = "Wired";
            else
                root.net = "";
        }
    }

    P5Support.DataSource {
        id: run
        engine: "executable"
        onNewData: (source, data) => disconnectSource(source)
    }

    component Glyph: Shape {
        id: g
        property string d
        property color tint: "#9aa3a8"
        width: 16
        height: 16
        layer.enabled: true
        layer.samples: 4
        ShapePath {
            strokeColor: g.tint
            strokeWidth: 2 * (g.width / 24)
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            scale: Qt.size(g.width / 24, g.height / 24)
            PathSvg { path: g.d }
        }
    }

    fullRepresentation: Item {
        id: cell
        Layout.minimumWidth: 68
        Layout.preferredWidth: 68
        Layout.minimumHeight: col.implicitHeight + 20
        Layout.preferredHeight: col.implicitHeight + 20

        Rectangle {
            anchors { fill: parent; leftMargin: 8; rightMargin: 8; topMargin: 2; bottomMargin: 2 }
            radius: 14
            color: "#ffffff"
            opacity: press.containsMouse ? 0.07 : 0
            Behavior on opacity { NumberAnimation { duration: 150; easing.type: Easing.OutCubic } }
        }
        MouseArea {
            id: press
            anchors.fill: parent
            hoverEnabled: true
            onClicked: run.connectSource("tripppin-ui quick")
        }

        Column {
            id: col
            anchors.centerIn: parent
            spacing: 8

            Glyph {
                visible: root.net !== ""
                anchors.horizontalCenter: parent.horizontalCenter
                d: root.net === "Wi-Fi" ? root.wifiPath : root.plugPath
            }
            Glyph {
                anchors.horizontalCenter: parent.horizontalCenter
                d: root.muted ? root.mutedPath : root.volumePath
                tint: root.muted ? "#ff6b6b" : "#9aa3a8"
            }
            Text {
                visible: battery.hasInternalBatteries
                anchors.horizontalCenter: parent.horizontalCenter
                horizontalAlignment: Text.AlignHCenter
                color: "#f1f3f4"
                font.family: "Manrope"
                font.pixelSize: 11
                font.weight: Font.Bold
                textFormat: Text.PlainText
                text: battery.percent + "%"
            }
            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                horizontalAlignment: Text.AlignHCenter
                color: "#f1f3f4"
                font.family: "Manrope"
                font.pixelSize: 15
                font.weight: Font.ExtraBold
                lineHeight: 1.05
                textFormat: Text.PlainText
                text: Qt.formatTime(root.now, "HH") + "\n" + Qt.formatTime(root.now, "mm")
            }
        }
    }
}
