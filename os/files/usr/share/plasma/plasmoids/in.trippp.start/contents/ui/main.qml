import QtQuick
import QtQuick.Layouts
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

        // The mark (mark.svg: the logo's own paths, viewBox "27 12 146 175"), 20 px tall. An
        // SVG is rendered at the screen's pixel density, so it stays crisp at any scale.
        Image {
            anchors.centerIn: parent
            source: "mark.svg"
            sourceSize.height: 20
            fillMode: Image.PreserveAspectFit
            smooth: true
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
