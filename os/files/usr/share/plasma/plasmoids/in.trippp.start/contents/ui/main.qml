import QtQuick
import QtQuick.Layouts
import QtQuick.Shapes
import org.kde.plasma.plasmoid
import org.kde.plasma.plasma5support as P5Support

// The taskbar's start button (mock section 02): a 40 x 40 accent square with the white
// mark. One press runs `tripppin-ui start`, the start screen. The mark is drawn from the
// logo's own path data (brand/logo.svg), so it is vector at any scale.
PlasmoidItem {
    id: root
    preferredRepresentation: fullRepresentation

    fullRepresentation: Item {
        id: cell
        Layout.minimumWidth: 40
        Layout.preferredWidth: 40
        Layout.maximumWidth: 40
        Layout.minimumHeight: 40
        Layout.preferredHeight: 40

        Rectangle {
            anchors.fill: parent
            color: press.pressed ? "#16553a" : "#1f6b45"
            Behavior on color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }
        }
        Rectangle {
            anchors.fill: parent
            color: "#ffffff"
            opacity: press.containsMouse && !press.pressed ? 0.12 : 0
            Behavior on opacity { NumberAnimation { duration: 150; easing.type: Easing.OutCubic } }
        }

        // The mark: the logo's own geometry, viewBox "27 12 146 175", drawn 20 px tall. The
        // Shape works in the logo's units and is scaled and shifted into place.
        Item {
            id: mark
            readonly property real s: 20 / 175
            anchors.centerIn: parent
            width: 146 * s
            height: 175 * s
            Shape {
                x: -27 * mark.s
                y: -12 * mark.s
                width: 200
                height: 200
                scale: mark.s
                transformOrigin: Item.TopLeft
                layer.enabled: true
                layer.samples: 4
                ShapePath {
                    strokeColor: "#ffffff"
                    strokeWidth: 24
                    fillColor: "transparent"
                    capStyle: ShapePath.RoundCap
                    PathSvg { path: "M58.32 70.84 A60 60 0 1 0 141.68 70.84" }
                }
                ShapePath {
                    strokeColor: "transparent"
                    fillColor: "#ffffff"
                    PathSvg { path: "M100 14 106.11 43.89 136 50 106.11 56.11 100 86 93.89 56.11 64 50 93.89 43.89Z" }
                }
            }
        }

        MouseArea {
            id: press
            anchors.fill: parent
            hoverEnabled: true
            onClicked: run.connectSource("tripppin-ui start")
        }
    }

    P5Support.DataSource {
        id: run
        engine: "executable"
        onNewData: (source, data) => disconnectSource(source)
    }

    Plasmoid.onActivated: run.connectSource("tripppin-ui start")
}
