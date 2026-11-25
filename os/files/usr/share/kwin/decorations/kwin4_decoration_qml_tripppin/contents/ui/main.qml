import QtQuick
import org.kde.kwin.decoration

// Tripppin "Bento" window frame: no border and no outline, a 38 px title strip with 14 px
// rounded top corners (Fedora has no KWin rounded-corner effect, so the bottom corners of the
// window stay square). Title bar #1f2225 while active, #1c1f21 when not; Manrope 600 title;
// three small round glyph buttons at the right (minimise, maximise, close) that turn amber on
// hover. Everything is drawn as vectors (rectangles and rotated bars), crisp at any scale;
// colours fade over 180 ms.
Decoration {
    id: root
    alpha: true

    readonly property int strip: 38
    readonly property int radius: 14
    readonly property bool active: decoration.client.active
    readonly property color fill: active ? "#1f2225" : "#1c1f21"
    readonly property color ink: active ? "#f1f3f4" : "#9aa3a8"
    readonly property color amber: "#f2b84b"

    DecorationOptions {
        id: options
        deco: decoration
    }

    Rectangle {
        id: bar
        anchors { left: parent.left; right: parent.right; top: parent.top }
        height: root.strip
        color: root.fill
        topLeftRadius: decoration.client.maximized ? 0 : root.radius
        topRightRadius: decoration.client.maximized ? 0 : root.radius
        Behavior on color { ColorAnimation { duration: 180; easing.type: Easing.OutCubic } }

        Text {
            id: caption
            anchors {
                left: parent.left
                leftMargin: 16
                right: buttons.left
                rightMargin: 12
                verticalCenter: parent.verticalCenter
            }
            text: decoration.client.caption
            textFormat: Text.PlainText
            elide: Text.ElideRight
            color: root.ink
            font.family: "Manrope"
            font.pixelSize: 13
            font.weight: Font.DemiBold
            Behavior on color { ColorAnimation { duration: 180; easing.type: Easing.OutCubic } }
        }

        // The window is moved and its menu opened through the title item.
        Component.onCompleted: decoration.installTitleItem(bar)

        Row {
            id: buttons
            anchors { right: parent.right; rightMargin: 10; verticalCenter: parent.verticalCenter }
            spacing: 6
            Repeater {
                model: [
                    { type: DecorationOptions.DecorationButtonMinimize, glyph: "min" },
                    { type: DecorationOptions.DecorationButtonMaximizeRestore, glyph: "max" },
                    { type: DecorationOptions.DecorationButtonClose, glyph: "close" }
                ]
                delegate: DecorationButton {
                    id: btn
                    required property var modelData
                    readonly property bool lit: btn.hovered || btn.pressed
                    readonly property color glyphColor: lit ? "#16181a" : root.ink
                    buttonType: modelData.type
                    width: 24
                    height: 24
                    Rectangle {
                        anchors.centerIn: parent
                        width: 22
                        height: 22
                        radius: 11
                        color: btn.lit ? root.amber : (root.active ? "#2e3337" : "#262a2d")
                        scale: btn.pressed ? 0.9 : 1
                        Behavior on color { ColorAnimation { duration: 150; easing.type: Easing.OutCubic } }
                        Behavior on scale { NumberAnimation { duration: 120; easing.type: Easing.OutCubic } }
                    }
                    // Glyphs, 8 px, 2 px round-capped strokes, centred in the 24 px cell.
                    Item {
                        anchors.centerIn: parent
                        width: 8
                        height: 8
                        Rectangle { visible: btn.modelData.glyph === "min"; x: 0; y: 3; width: 8; height: 2; radius: 1; color: btn.glyphColor }
                        Rectangle {
                            visible: btn.modelData.glyph === "max"
                            anchors.fill: parent
                            radius: 2
                            color: "transparent"
                            border { width: 2; color: btn.glyphColor }
                        }
                        Rectangle {
                            visible: btn.modelData.glyph === "close"
                            anchors.centerIn: parent
                            width: 11; height: 2; radius: 1
                            rotation: 45
                            color: btn.glyphColor
                        }
                        Rectangle {
                            visible: btn.modelData.glyph === "close"
                            anchors.centerIn: parent
                            width: 11; height: 2; radius: 1
                            rotation: -45
                            color: btn.glyphColor
                        }
                    }
                }
            }
        }
    }

    Component.onCompleted: {
        borders.setBorders(0);
        borders.setTitle(strip);
        maximizedBorders.setBorders(0);
        maximizedBorders.setTitle(strip);
        extendedBorders.setBorders(4);
    }
}
