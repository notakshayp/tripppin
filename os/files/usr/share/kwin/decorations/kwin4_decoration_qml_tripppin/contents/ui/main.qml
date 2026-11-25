import QtQuick
import org.kde.kwin.decoration

// Tripppin "Tiles" window frame (design mock section 05): square, flat, no border and no
// shadow. A 36 px strip across the top: accent #1f6b45 with white text while the window is
// active, raised #1a2025 with muted text when it is not. Flat vector glyph buttons at the
// right: minimise, maximise, close, all drawn from rectangles so they stay crisp at any
// scale. Colours fade over 180 ms.
Decoration {
    id: root
    alpha: false

    readonly property int strip: 36
    readonly property bool active: decoration.client.active
    readonly property color fill: active ? "#1f6b45" : "#1a2025"
    readonly property color ink: active ? "#ffffff" : "#9aa4ab"

    DecorationOptions {
        id: options
        deco: decoration
    }

    Rectangle {
        id: bar
        anchors { left: parent.left; right: parent.right; top: parent.top }
        height: root.strip
        color: root.fill
        Behavior on color { ColorAnimation { duration: 180; easing.type: Easing.OutCubic } }

        Text {
            id: caption
            anchors {
                left: parent.left
                leftMargin: 14
                right: buttons.left
                rightMargin: 14
                verticalCenter: parent.verticalCenter
            }
            text: decoration.client.caption
            textFormat: Text.PlainText
            elide: Text.ElideRight
            color: root.ink
            font.family: "Inter"
            font.pixelSize: 14
            font.weight: Font.DemiBold
            Behavior on color { ColorAnimation { duration: 180; easing.type: Easing.OutCubic } }
        }

        // The window is moved and its menu opened through the title item.
        Component.onCompleted: decoration.installTitleItem(bar)

        Row {
            id: buttons
            anchors { right: parent.right; top: parent.top }
            height: root.strip
            Repeater {
                model: [
                    { type: DecorationOptions.DecorationButtonMinimize, glyph: "min" },
                    { type: DecorationOptions.DecorationButtonMaximizeRestore, glyph: "max" },
                    { type: DecorationOptions.DecorationButtonClose, glyph: "close" }
                ]
                delegate: DecorationButton {
                    id: btn
                    required property var modelData
                    buttonType: modelData.type
                    width: root.strip
                    height: root.strip
                    Rectangle {
                        anchors.fill: parent
                        color: btn.modelData.glyph === "close" ? "#c2502e" : "#ffffff"
                        opacity: btn.pressed ? 0.35 : (btn.hovered ? (btn.modelData.glyph === "close" ? 1 : 0.18) : 0)
                        Behavior on opacity { NumberAnimation { duration: 150; easing.type: Easing.OutCubic } }
                    }
                    // Glyphs, 12 px, 1.5 px strokes, centred in the 36 px cell.
                    Item {
                        anchors.centerIn: parent
                        width: 12
                        height: 12
                        Rectangle { visible: btn.modelData.glyph === "min"; x: 0; y: 10; width: 12; height: 1.5; color: root.ink }
                        Rectangle {
                            visible: btn.modelData.glyph === "max"
                            anchors.fill: parent
                            color: "transparent"
                            border { width: 1.5; color: root.ink }
                        }
                        Rectangle {
                            visible: btn.modelData.glyph === "close"
                            anchors.centerIn: parent
                            width: 16.5; height: 1.5
                            rotation: 45
                            color: btn.hovered ? "#ffffff" : root.ink
                        }
                        Rectangle {
                            visible: btn.modelData.glyph === "close"
                            anchors.centerIn: parent
                            width: 16.5; height: 1.5
                            rotation: -45
                            color: btn.hovered ? "#ffffff" : root.ink
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
