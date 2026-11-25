import QtQuick
import QtQuick.Shapes

// The login splash: #101418, then the spark (a green four-point star, upright, never
// turning) fades in, then the ring draws itself. Geometry is the mark's
// (crates/trip/src/logo.rs, brand/logo-animated.svg): a 146 x 175 box from (27,12), ring
// radius 60 centred at (100,114), stroke 22, the spark from (100,14) to (100,86). Drawn as
// vector Shapes, so it is crisp at any scale and pixel density; the ring is an arc whose
// sweep animates.
Rectangle {
    id: root
    color: "#101418"
    property int stage

    Item {
        id: mark
        anchors.centerIn: parent
        // The mark is 175 units tall; it fills a fifth of the screen height, at most 220 px.
        readonly property real unit: Math.min(root.height * 0.2, 220) / 175
        width: 146 * unit
        height: 175 * unit
        property real ring: 0
        property real sparkOpacity: 0

        Shape {
            id: art
            // Local units are the mark's own; the Shape is scaled and shifted into place.
            x: -27 * mark.unit
            y: -12 * mark.unit
            width: 200
            height: 200
            scale: mark.unit
            transformOrigin: Item.TopLeft
            layer.enabled: true
            layer.samples: 4

            ShapePath {
                strokeColor: mark.ring > 0 ? "#ffffff" : "transparent"
                strokeWidth: 22
                fillColor: "transparent"
                capStyle: ShapePath.RoundCap
                // From the upper left (58.32,70.84) round the bottom to the upper right,
                // 272 degrees in all: the arc's own start and sweep.
                PathAngleArc {
                    centerX: 100
                    centerY: 114
                    radiusX: 60
                    radiusY: 60
                    startAngle: 134.0
                    sweepAngle: 272.0 * mark.ring
                }
            }
            ShapePath {
                strokeColor: "transparent"
                fillColor: Qt.rgba(0.247, 0.682, 0.478, mark.sparkOpacity)
                PathSvg { path: "M100 14 106.11 43.89 136 50 106.11 56.11 100 86 93.89 56.11 64 50 93.89 43.89Z" }
            }
        }
    }

    SequentialAnimation {
        running: true
        NumberAnimation { target: mark; property: "sparkOpacity"; from: 0; to: 1; duration: 450; easing.type: Easing.OutCubic }
        NumberAnimation { target: mark; property: "ring"; from: 0; to: 1; duration: 900; easing.type: Easing.OutCubic }
    }
}
