import QtQuick
import QtQuick.Shapes

// The Tripppin lock screen (design mock section 01, "Tiles"), in two stages like the tile
// desktops' curtain.
//
// Stage 1 is the mock: flat teal with a darkening towards the bottom, the big light time
// bottom-left, the date, the next event and the unread count per app when there are any,
// "Swipe up or type" bottom-right, Wi-Fi and battery top-right. No password field.
// Any key, a click or an upward swipe slides the whole curtain up (250 ms, ease-out) to
// stage 2: a square avatar, the full name in 30 px light, and a flat square password box with
// an arrow button. A typed first character goes into the box. Esc, or 10 s without input,
// drops the curtain again. Everything is vector (text, Shapes, scalable icons), so it is
// crisp at any pixel density.
//
// This replaces Plasma's own LockScreenUi.qml in the shell package (Plasma 6.8 takes the lock
// screen from the shell package, not from the look-and-feel one); the image copies it over
// the stock file. The outer LockScreen.qml (kept as is) provides `root` with `notification`,
// `clearPassword` and `viewVisible`. Optional parts (Wi-Fi, battery, event, badges, full
// name) are loaded or read on their own, so a missing module hides that part, not the screen.
Item {
    id: ui
    implicitWidth: 800
    implicitHeight: 600

    // The mock is 1000 x 625: the screen is that picture scaled to its height, never smaller.
    readonly property real s: Math.max(1, height / 625)
    property bool open: false
    property string pendingPassword: ""
    property string fullName: ""
    readonly property color ink: "#ffffff"
    readonly property color muted: "#9aa4ab"
    readonly property string login: typeof kscreenlocker_userName !== "undefined" ? kscreenlocker_userName : ""
    readonly property string face: (typeof kscreenlocker_userImage !== "undefined" && kscreenlocker_userImage !== "")
        ? "file://" + kscreenlocker_userImage.split("/").map(encodeURIComponent).join("/") : ""

    function raise(typed) {
        if (!open) {
            open = true;
            authenticator.startAuthenticating();
        }
        field.forceActiveFocus();
        if (typed && typed.length > 0) field.text += typed;
        idle.restart();
    }
    function drop() {
        open = false;
        field.text = "";
        root.notification = "";
        keys.forceActiveFocus();
    }

    Connections {
        target: root
        function onClearPassword() { field.text = "" }
    }

    // Stage 2 sits behind the curtain on the plain background.
    Rectangle {
        anchors.fill: parent
        color: "#101418"
    }

    Item {
        id: stage2
        anchors.fill: parent
        opacity: ui.open ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: 250; easing.type: Easing.OutCubic } }

        Column {
            anchors.centerIn: parent
            spacing: 0

            // The avatar: the person's picture when there is one, else the initial on an
            // accent tile. Square.
            Item {
                anchors.horizontalCenter: parent.horizontalCenter
                width: 120 * ui.s
                height: 120 * ui.s
                Rectangle {
                    anchors.fill: parent
                    color: "#1f6b45"
                    visible: face.status !== Image.Ready
                    Text {
                        anchors.centerIn: parent
                        text: (ui.fullName || ui.login).charAt(0).toUpperCase()
                        color: ui.ink
                        font.family: "Inter"
                        font.weight: Font.Light
                        font.pixelSize: 64 * ui.s
                    }
                }
                Image {
                    id: face
                    anchors.fill: parent
                    source: ui.face
                    sourceSize: Qt.size(width * 2, height * 2)
                    fillMode: Image.PreserveAspectCrop
                    smooth: true
                    visible: status === Image.Ready
                }
            }
            Item { width: 1; height: 24 * ui.s }
            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: ui.fullName !== "" ? ui.fullName : ui.login
                color: ui.ink
                font.family: "Inter"
                font.weight: Font.Light
                font.pixelSize: 30 * ui.s
            }
            Item { width: 1; height: 28 * ui.s }

            Item {
                id: form
                anchors.horizontalCenter: parent.horizontalCenter
                width: 380 * ui.s
                height: 56 * ui.s
                transform: Translate { id: shiftX; x: 0 }
                SequentialAnimation {
                    id: shake
                    NumberAnimation { target: shiftX; property: "x"; to: -12; duration: 50; easing.type: Easing.OutCubic }
                    NumberAnimation { target: shiftX; property: "x"; to: 10; duration: 80; easing.type: Easing.InOutCubic }
                    NumberAnimation { target: shiftX; property: "x"; to: -6; duration: 80; easing.type: Easing.InOutCubic }
                    NumberAnimation { target: shiftX; property: "x"; to: 0; duration: 60; easing.type: Easing.OutCubic }
                }

                Rectangle {
                    anchors { left: parent.left; top: parent.top; bottom: parent.bottom; right: go.left }
                    color: "#1a2025"
                    border.width: 2
                    border.color: field.activeFocus ? "#3fae7a" : "#3a444b"
                    Behavior on border.color { ColorAnimation { duration: 150; easing.type: Easing.OutCubic } }
                }
                TextInput {
                    id: field
                    anchors { left: parent.left; right: go.left; top: parent.top; bottom: parent.bottom
                              leftMargin: 16 * ui.s; rightMargin: 16 * ui.s }
                    verticalAlignment: TextInput.AlignVCenter
                    echoMode: TextInput.Password
                    passwordCharacter: "•"
                    color: ui.ink
                    selectionColor: "#1f6b45"
                    font.family: "Inter"
                    font.weight: Font.Light
                    font.pixelSize: 20 * ui.s
                    clip: true
                    onAccepted: ui.submit()
                    onTextChanged: idle.restart()
                    Keys.onEscapePressed: ui.drop()
                    Text {
                        visible: field.text.length === 0
                        anchors.verticalCenter: parent.verticalCenter
                        text: "Password"
                        color: ui.muted
                        font: field.font
                    }
                }
                // The arrow button, flat and square.
                Rectangle {
                    id: go
                    anchors { right: parent.right; top: parent.top; bottom: parent.bottom }
                    width: height
                    color: goArea.pressed ? "#16553a" : (goArea.containsMouse ? "#2a7b54" : "#1f6b45")
                    Behavior on color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }
                    Shape {
                        id: arrow
                        anchors.centerIn: parent
                        width: 24 * ui.s
                        height: 24 * ui.s
                        layer.enabled: true
                        layer.samples: 4
                        readonly property real k: width / 24
                        ShapePath {
                            strokeColor: "#ffffff"
                            strokeWidth: 1.8 * arrow.k
                            fillColor: "transparent"
                            capStyle: ShapePath.RoundCap
                            joinStyle: ShapePath.RoundJoin
                            PathPolyline {
                                path: [Qt.point(4 * arrow.k, 12 * arrow.k), Qt.point(20 * arrow.k, 12 * arrow.k),
                                       Qt.point(14 * arrow.k, 6 * arrow.k), Qt.point(20 * arrow.k, 12 * arrow.k),
                                       Qt.point(14 * arrow.k, 18 * arrow.k)]
                            }
                        }
                    }
                    MouseArea {
                        id: goArea
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: ui.submit()
                    }
                }
            }
            Item { width: 1; height: 12 * ui.s }
            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: root.notification
                color: "#e8825c"
                font.family: "Inter"
                font.pixelSize: 14 * ui.s
                opacity: text.length > 0 ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 150 } }
            }
        }
    }

    function submit() {
        if (field.text.length === 0) return;
        if (authenticator.pamTimeout) {
            pendingPassword = field.text;
            field.enabled = false;
        } else {
            authenticator.respond(field.text);
        }
    }

    Connections {
        target: authenticator
        function onFailed(kind) {
            if (kind !== 0) return;
            root.notification = "Wrong password";
            field.text = "";
            field.enabled = true;
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

    Timer {
        id: idle
        interval: 10000
        onTriggered: if (ui.open && field.text.length === 0) ui.drop()
    }

    // Stage 1: the curtain. It slides up off the screen when raised.
    Item {
        id: curtain
        width: parent.width
        height: parent.height
        y: ui.open ? -height : 0
        Behavior on y { NumberAnimation { duration: 250; easing.type: Easing.OutCubic } }
        opacity: 0
        NumberAnimation on opacity { from: 0; to: 1; duration: 250; easing.type: Easing.OutCubic; running: true }

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

        // Time, date, next event, unread counts: bottom-left.
        Column {
            id: block
            x: 48 * ui.s
            y: curtain.height - height - 60 * ui.s

            Text {
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
            Loader {
                id: extras
                source: "Extras.qml"
                onLoaded: item.s = Qt.binding(function () { return ui.s })
            }
        }

        // "Swipe up or type", bottom-right, with the chevron.
        Column {
            anchors { right: parent.right; rightMargin: 40 * ui.s; bottom: parent.bottom; bottomMargin: 60 * ui.s }
            spacing: 6 * ui.s
            Item {
                anchors.horizontalCenter: parent.horizontalCenter
                width: 22 * ui.s
                height: 22 * ui.s
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
            anchors { right: parent.right; rightMargin: 40 * ui.s; top: parent.top; topMargin: 30 * ui.s }
            source: "Status.qml"
            onLoaded: item.s = Qt.binding(function () { return ui.s })
        }
    }

    // Input. A press or an upward drag raises the curtain; so does any key. A first printable
    // character (not Space or Enter, which only raise it) goes into the box.
    MouseArea {
        id: keys
        anchors.fill: parent
        hoverEnabled: true
        focus: true
        property real startY: -1
        onPressed: mouse => { startY = mouse.y; if (!ui.open) ui.raise("") }
        onPositionChanged: mouse => {
            if (ui.open) idle.restart();
            else if (startY >= 0 && startY - mouse.y > 40 * ui.s) ui.raise("");
        }
        onReleased: startY = -1
        Keys.onPressed: event => {
            if (event.key === Qt.Key_Escape) {
                ui.drop();
            } else if (event.key === Qt.Key_Shift || event.key === Qt.Key_Control || event.key === Qt.Key_Alt
                       || event.key === Qt.Key_Meta || event.key === Qt.Key_CapsLock) {
                // Modifiers alone do nothing.
            } else {
                var printable = event.text.length > 0 && event.text.charCodeAt(0) >= 32
                    && event.key !== Qt.Key_Space && event.key !== Qt.Key_Return && event.key !== Qt.Key_Enter;
                ui.raise(printable ? event.text : "");
            }
            event.accepted = true;
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

    // The person's full name, when the system has one (the first comma-separated part of the
    // passwd entry's name field).
    Loader {
        source: "FullName.qml"
        onLoaded: item.result.connect(function (n) { ui.fullName = n })
    }

    Component.onCompleted: keys.forceActiveFocus()
}
