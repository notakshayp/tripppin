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
            anchors.fill: parent
            // Coordinates are the mark's own units times `unit`, so the Shape is drawn at
            // its real pixel size (and, as a layer, at the screen's pixel density) rather
            // than scaled up from a small texture.
            layer.enabled: true
            layer.samples: 4
            readonly property real k: mark.unit

            ShapePath {
                strokeColor: mark.ring > 0 ? "#ffffff" : "transparent"
                strokeWidth: 22 * art.k
                fillColor: "transparent"
                capStyle: ShapePath.RoundCap
                // From the upper left (58.32,70.84) round the bottom to the upper right,
                // 272 degrees in all, counter-clockwise (QML angles run clockwise from 3 o clock).
                PathAngleArc {
                    centerX: (100 - 27) * art.k
                    centerY: (114 - 12) * art.k
                    radiusX: 60 * art.k
                    radiusY: 60 * art.k
                    startAngle: -134.0
                    sweepAngle: -272.0 * mark.ring
                }
            }
            ShapePath {
                strokeColor: "transparent"
                fillColor: Qt.rgba(0.247, 0.682, 0.478, mark.sparkOpacity)
                PathPolyline {
                    path: [[100, 14], [106.11, 43.89], [136, 50], [106.11, 56.11], [100, 86],
                           [93.89, 56.11], [64, 50], [93.89, 43.89], [100, 14]].map(function (p) {
                        return Qt.point((p[0] - 27) * art.k, (p[1] - 12) * art.k);
                    })
                }
            }
        }
    }

    SequentialAnimation {
        running: true
        NumberAnimation { target: mark; property: "sparkOpacity"; from: 0; to: 1; duration: 450; easing.type: Easing.OutCubic }
        NumberAnimation { target: mark; property: "ring"; from: 0; to: 1; duration: 900; easing.type: Easing.OutCubic }
    }
}
