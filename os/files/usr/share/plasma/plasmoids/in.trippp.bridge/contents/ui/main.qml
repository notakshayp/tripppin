import QtQuick
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.plasma5support as P5Support

// The rail's top button (Bento mock 02): a 42 x 42 amber tile with 14 px corners and the dark
// mark. One press runs `tripppin-ui bridge` (the Meta key does the same through the global
// shortcut). The mark is drawn from the logo's own path data (mark.svg, brand/logo.svg), so it
// is vector at any scale.
PlasmoidItem {
    id: root
    preferredRepresentation: fullRepresentation

    fullRepresentation: Item {
        id: cell
        Layout.minimumWidth: 68
        Layout.preferredWidth: 68
        Layout.minimumHeight: 80
        Layout.preferredHeight: 80

        Rectangle {
            id: tile
            anchors { horizontalCenter: parent.horizontalCenter; top: parent.top; topMargin: 14 }
            width: 42
            height: 42
            radius: 14
            color: press.pressed ? "#d9a23c" : (press.containsMouse ? "#f7cc77" : "#f2b84b")
            scale: press.pressed ? 0.94 : 1
            Behavior on color { ColorAnimation { duration: 150; easing.type: Easing.OutCubic } }
            Behavior on scale { NumberAnimation { duration: 150; easing.type: Easing.OutCubic } }

            // The mark (mark.svg: the logo's own paths, viewBox "27 12 146 175"), 24 px tall.
            // An SVG is rendered at the screen's pixel density, so it stays crisp at any scale.
            Image {
                anchors.centerIn: parent
                source: "mark.svg"
                sourceSize.height: 24
                fillMode: Image.PreserveAspectFit
                smooth: true
            }
        }

        // The short divider under the tile.
        Rectangle {
            anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: 8 }
            width: 28
            height: 1
            color: "#2a2e31"
        }

        MouseArea {
            id: press
            anchors { left: parent.left; right: parent.right; top: parent.top; bottom: parent.bottom }
            hoverEnabled: true
            onClicked: run.connectSource("tripppin-ui bridge")
        }
    }

    P5Support.DataSource {
        id: run
        engine: "executable"
        onNewData: (source, data) => disconnectSource(source)
    }

    Plasmoid.onActivated: run.connectSource("tripppin-ui bridge")
}
