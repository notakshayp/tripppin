import QtQuick
import QtQuick.Shapes

// The Tripppin lock screen (design mock section 01, "Tiles"): flat teal, a big light time
// bottom-left with the date, "Swipe up or type" bottom-right,
// Wi-Fi and battery top-right. Typing, a click or an upward swipe brings in the password
// field in the same style. Everything is vector (text, Shapes, scalable icons), so it is
// crisp at any pixel density; every change eases over 150 to 250 ms.
//
// This replaces Plasma's own LockScreenUi.qml in the shell package (Plasma 6.8 takes the lock
// screen from the shell package, not from the look-and-feel one); the image copies it over
// the stock file. The outer LockScreen.qml (kept as is) provides `root` with `notification`,
// `clearPassword` and `viewVisible`. Optional parts (Wi-Fi, battery) are
// loaded on their own, so a missing module hides that part instead of the screen.
Item {
    id: ui

    implicitWidth: 800
    implicitHeight: 600

    // The mock is 1000 x 625: the screen is that picture scaled to its height, never smaller.
    readonly property real s: Math.max(1, height / 625)
    property bool askPassword: false
    property string pendingPassword: ""
    readonly property color ink: "#ffffff"
    readonly property color muted: "#9aa4ab"

    function reveal() {
        askPassword = true;
        authenticator.startAuthenticating();
        field.forceActiveFocus();
        hideTimer.restart();
    }
    function conceal() {
        askPassword = false;
        field.text = "";
        root.notification = "";
        ui.forceActiveFocus();
    }

    Connections {
        target: root
        function onClearPassword() { field.text = "" }
    }

    Rectangle {
        anchors.fill: parent
        color: "#0f3d3a"
    }
    Rectangle {
        anchors.fill: parent
        gradient: Gradient {
            GradientStop { position: 0.4; color: "#00000000" }
            GradientStop { position: 1.0; color: "#73000000" }
        }
    }

    opacity: 0
    NumberAnimation on opacity { from: 0; to: 1; duration: 250; easing.type: Easing.OutCubic; running: true }

    // Taps and typing anywhere bring in the field; an upward drag too.
    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        focus: true
        property real startY: -1
        onPressed: mouse => { startY = mouse.y; ui.reveal() }
        onPositionChanged: mouse => { if (startY >= 0 && startY - mouse.y > 40 * ui.s) ui.reveal() }
        onReleased: startY = -1
        Keys.onPressed: event => {
            if (event.key === Qt.Key_Escape) {
                ui.conceal();
            } else {
                ui.reveal();
            }
            event.accepted = false;
        }
    }

    Timer {
        id: hideTimer
        interval: 15000
        onTriggered: if (field.text.length === 0) ui.conceal()
    }

    Connections {
        target: authenticator
        function onFailed(kind) {
            if (kind !== 0) return;
            root.notification = "Wrong password";
            field.text = "";
            authenticator.startAuthenticating();
            shake.start();
        }
        function onSucceeded() {
            authenticator.stopAuthenticating();
            Qt.quit();
        }
        function onInfoMessageChanged() { if (authenticator.infoMessage) root.notification = authenticator.infoMessage }
        function onErrorMessageChanged() { if (authenticator.errorMessage) root.notification = authenticator.errorMessage }
        function onPamTimeoutChanged() {
            if (!authenticator.pamTimeout && ui.pendingPassword.length > 0) {
                authenticator.respond(ui.pendingPassword);
                ui.pendingPassword = "";
                field.enabled = true;
            }
        }
    }

    // Time, date, unread counts: bottom-left, lifting when the password field appears.
    Column {
        id: block
        x: 48 * ui.s
        y: ui.height - height - 60 * ui.s - (ui.askPassword ? 76 * ui.s : 0)
        Behavior on y { NumberAnimation { duration: 220; easing.type: Easing.OutCubic } }

        Text {
            id: time
            color: ui.ink
            font.family: "Inter"
            font.weight: Font.ExtraLight
            font.pixelSize: 140 * ui.s
            font.letterSpacing: -0.02 * 140 * ui.s
            lineHeight: 1.0
            text: Qt.formatTime(clock.now, "HH:mm")
        }
        Item { width: 1; height: 4 * ui.s }
        Text {
            color: ui.ink
            font.family: "Inter"
            font.weight: Font.Light
            font.pixelSize: 30 * ui.s
            text: Qt.formatDate(clock.now, "dddd, d MMMM")
        }
    }

    QtObject {
        id: clock
        property date now: new Date()
    }
    Timer {
        interval: 1000
        running: true
        repeat: true
        onTriggered: clock.now = new Date()
    }

    // The password field, in the same flat style: a raised box, a 2 px outline that turns
    // accent green while it has focus.
    Item {
        id: form
        x: 48 * ui.s
        y: ui.height - 60 * ui.s - height + 4 * ui.s
        width: 360 * ui.s
        height: 56 * ui.s
        opacity: ui.askPassword ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: 200; easing.type: Easing.OutCubic } }
        transform: Translate { id: shiftX; x: 0 }

        SequentialAnimation {
            id: shake
            NumberAnimation { target: shiftX; property: "x"; to: -12; duration: 50; easing.type: Easing.OutCubic }
            NumberAnimation { target: shiftX; property: "x"; to: 10; duration: 80; easing.type: Easing.InOutCubic }
            NumberAnimation { target: shiftX; property: "x"; to: -6; duration: 80; easing.type: Easing.InOutCubic }
            NumberAnimation { target: shiftX; property: "x"; to: 0; duration: 60; easing.type: Easing.OutCubic }
        }

        Rectangle {
            anchors.fill: parent
            color: "#a0101418"
            border.width: 2
            border.color: field.activeFocus ? "#3fae7a" : "#3a444b"
            Behavior on border.color { ColorAnimation { duration: 150; easing.type: Easing.OutCubic } }
        }
        TextInput {
            id: field
            anchors { fill: parent; leftMargin: 16 * ui.s; rightMargin: 16 * ui.s }
            verticalAlignment: TextInput.AlignVCenter
            echoMode: TextInput.Password
            passwordCharacter: "•"
            color: ui.ink
            selectionColor: "#1f6b45"
            font.family: "Inter"
            font.weight: Font.Light
            font.pixelSize: 20 * ui.s
            clip: true
            onAccepted: {
                if (text.length === 0) return;
                if (authenticator.pamTimeout) {
                    ui.pendingPassword = text;
                    enabled = false;
                } else {
                    authenticator.respond(text);
                }
            }
            onTextChanged: hideTimer.restart()
            Keys.onEscapePressed: ui.conceal()
            Text {
                visible: field.text.length === 0
                anchors.verticalCenter: parent.verticalCenter
                text: "Password"
                color: ui.muted
                font: field.font
            }
        }
        Text {
            anchors { left: parent.left; top: parent.bottom; topMargin: 8 * ui.s }
            text: root.notification
            color: "#e8825c"
            font.family: "Inter"
            font.pixelSize: 14 * ui.s
            opacity: text.length > 0 ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: 150 } }
        }
    }

    // "Swipe up or type", bottom-right.
    Column {
        id: hint
        anchors { right: parent.right; rightMargin: 40 * ui.s; bottom: parent.bottom; bottomMargin: 60 * ui.s }
        spacing: 6 * ui.s
        opacity: ui.askPassword ? 0 : 1
        Behavior on opacity { NumberAnimation { duration: 180; easing.type: Easing.OutCubic } }
        Item {
            anchors.horizontalCenter: parent.horizontalCenter
            width: 22 * ui.s
            height: 22 * ui.s
            // The chevron (a 24 x 24 box scaled to the item) as a real-size vector Shape.
            Shape {
                id: chev
                anchors.fill: parent
                layer.enabled: true
                layer.samples: 4
                readonly property real k: width / 24
                ShapePath {
                    strokeColor: ui.muted
                    strokeWidth: 1.5 * chev.k
                    fillColor: "transparent"
                    capStyle: ShapePath.RoundCap
                    joinStyle: ShapePath.RoundJoin
                    PathPolyline {
                        path: [Qt.point(6 * chev.k, 15 * chev.k), Qt.point(12 * chev.k, 9 * chev.k),
                               Qt.point(18 * chev.k, 15 * chev.k)]
                    }
                }
            }
        }
        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: "Swipe up or type"
            color: ui.muted
            font.family: "Inter"
            font.pixelSize: 13 * ui.s
        }
    }

    // Wi-Fi and battery, top-right.
    Loader {
        id: status
        anchors { right: parent.right; rightMargin: 40 * ui.s; top: parent.top; topMargin: 30 * ui.s }
        source: "Status.qml"
        onLoaded: item.s = Qt.binding(function () { return ui.s })
    }

    Component.onCompleted: area.forceActiveFocus()
}
