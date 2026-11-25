import QtQuick

// The login splash: ink, then the spark (a green four-point star, upright, never turning)
// fades in, then the ring draws itself. Geometry and colours are the mark's
// (crates/trip/src/logo.rs, brand/logo-animated.svg): a 146 x 175 box from (27,12), ring
// radius 60 centred at (100,114), stroke 22, the spark from (100,14) to (100,86).
Rectangle {
    id: root
    color: "#0b100f"
    property int stage

    Item {
        id: mark
        anchors.centerIn: parent
        property real unit: Math.min(root.height * 0.2, 220) / 175
        width: 146 * unit
        height: 175 * unit
        property real ring: 0
        property real sparkOpacity: 0
        onRingChanged: canvas.requestPaint()
        onSparkOpacityChanged: canvas.requestPaint()

        Canvas {
            id: canvas
            anchors.fill: parent
            antialiasing: true
            onPaint: {
                var ctx = getContext("2d");
                ctx.reset();
                var s = mark.unit;
                function X(x) { return (x - 27) * s; }
                function Y(y) { return (y - 12) * s; }
                if (mark.ring > 0) {
                    var start = Math.atan2(70.84 - 114, 58.32 - 100);
                    ctx.strokeStyle = "#f2f5ef";
                    ctx.lineWidth = 22 * s;
                    ctx.lineCap = "round";
                    ctx.beginPath();
                    ctx.arc(X(100), Y(114), 60 * s, start, start - mark.ring * 4.7473, true);
                    ctx.stroke();
                }
                ctx.globalAlpha = mark.sparkOpacity;
                ctx.fillStyle = "#57d99a";
                ctx.beginPath();
                var pts = [[100,14],[106.11,43.89],[136,50],[106.11,56.11],[100,86],[93.89,56.11],[64,50],[93.89,43.89]];
                ctx.moveTo(X(pts[0][0]), Y(pts[0][1]));
                for (var i = 1; i < pts.length; i++) ctx.lineTo(X(pts[i][0]), Y(pts[i][1]));
                ctx.closePath();
                ctx.fill();
            }
        }
    }

    SequentialAnimation {
        running: true
        NumberAnimation { target: mark; property: "sparkOpacity"; from: 0; to: 1; duration: 450; easing.type: Easing.OutCubic }
        NumberAnimation { target: mark; property: "ring"; from: 0; to: 1; duration: 900; easing.type: Easing.InOutCubic }
    }
}
