import QtQuick
import QtQuick.Layouts
import QtQuick.Shapes
import org.kde.kirigami as Kirigami

// The Tripppin lock screen (Bento mock section 01): a board of rounded tiles on #16181a, 12 px
// apart. Left: the big time tile (Manrope 800, about 120 px, with the logo) over the amber
// sign-in tile, where the password field lives. Right: the next event (blue), the battery and
// connection, what is playing, and the unread count, each only when there is real data.
//
// There is no curtain: the board is the whole screen and the amber tile's field already has the
// focus, so typing goes straight into it. Esc clears the field. The tiles slide up and fade in
// (staggered, 250 ms, ease-out). Everything is vector (text, Shapes, scalable icons), so it is
// crisp at any pixel density.
//
// This replaces Plasma's own LockScreenUi.qml in the shell package (Plasma 6.8 takes the lock
// screen from the shell package, not from the look-and-feel one). The outer LockScreen.qml
// (kept as is) provides `root` with `notification`, `clearPassword` and `viewVisible`. Optional
// parts (Wi-Fi, battery, event, playing, badges, full name) are loaded or read on their own, so
// a missing module hides that tile, not the screen.
Item {
    id: ui
    implicitWidth: 800
    implicitHeight: 600

    // The mock is 1000 x 625: the screen is that picture scaled to its height, never smaller.
    readonly property real s: Math.max(1, height / 625)
    property string pendingPassword: ""
    property string fullName: ""
    readonly property color ink: "#f1f3f4"
    readonly property color muted: "#9aa3a8"
    readonly property color amber: "#f2b84b"
    readonly property color dark: "#16181a"
    readonly property string family: "Manrope"
    readonly property string login: typeof kscreenlocker_userName !== "undefined" ? kscreenlocker_userName : ""
    readonly property string face: (typeof kscreenlocker_userImage !== "undefined" && kscreenlocker_userImage !== "")
        ? "file://" + kscreenlocker_userImage.split("/").map(encodeURIComponent).join("/") : ""
    readonly property string who: fullName !== "" ? fullName : login
    readonly property var extras: extrasLoader.item
    readonly property var status: statusLoader.item
    readonly property real rowH: (board.height - 24 * s) / 3

    Connections {
        target: root
        function onClearPassword() { field.text = "" }
    }

    Rectangle {
        anchors.fill: parent
        color: "#16181a"
    }

    Loader { id: extrasLoader; source: "Extras.qml" }
    Loader { id: statusLoader; source: "Status.qml" }

    // A tile: a 20 px rounded panel that slides up and fades in, `order` tiles after the first.
    component Tile: Rectangle {
        id: tile
        property int order: 0
        property real shiftX: 0
        radius: 20 * ui.s
        color: "#23272a"
        opacity: 0
        transform: [Translate { id: lift; y: 14 * ui.s }, Translate { x: tile.shiftX }]
        Component.onCompleted: appear.start()
        SequentialAnimation {
            id: appear
            PauseAnimation { duration: 60 * tile.order }
            ParallelAnimation {
                NumberAnimation { target: tile; property: "opacity"; to: 1; duration: 250; easing.type: Easing.OutCubic }
                NumberAnimation { target: lift; property: "y"; to: 0; duration: 250; easing.type: Easing.OutCubic }
            }
        }
    }

    RowLayout {
        id: board
        anchors { fill: parent; margins: 28 * ui.s }
        spacing: 12 * ui.s

        // Left half: time over sign-in.
        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.preferredWidth: 1
            spacing: 12 * ui.s

            Tile {
                order: 0
                color: "#2b3a33"
                Layout.fillWidth: true
                Layout.fillHeight: true
                Image {
                    anchors { left: parent.left; top: parent.top; margins: 16 * ui.s }
                    source: "mark.svg"
                    sourceSize.height: 34 * ui.s
                    fillMode: Image.PreserveAspectFit
                    smooth: true
                }
                Column {
                    anchors { left: parent.left; right: parent.right; bottom: parent.bottom; margins: 16 * ui.s }
                    spacing: 6 * ui.s
                    Text {
                        color: ui.ink
                        font.family: ui.family
                        font.weight: Font.ExtraBold
                        font.pixelSize: Math.min(120 * ui.s, parent.width / 2.7)
                        font.letterSpacing: -0.03 * font.pixelSize
                        lineHeight: 0.9
                        text: Qt.formatTime(clock.now, "HH:mm")
                    }
                    Text {
                        color: ui.muted
                        font.family: ui.family
                        font.weight: Font.DemiBold
                        font.pixelSize: 18 * ui.s
                        text: Qt.formatDate(clock.now, "dddd d MMMM")
                    }
                }
            }

            // The sign-in tile: avatar, name, the password field, the arrow.
            Tile {
                id: signin
                order: 4
                color: ui.amber
                Layout.fillWidth: true
                Layout.preferredHeight: ui.rowH
                Layout.minimumHeight: 84 * ui.s

                SequentialAnimation {
                    id: shake
                    NumberAnimation { target: signin; property: "shiftX"; to: -12; duration: 50; easing.type: Easing.OutCubic }
                    NumberAnimation { target: signin; property: "shiftX"; to: 10; duration: 80; easing.type: Easing.InOutCubic }
                    NumberAnimation { target: signin; property: "shiftX"; to: -6; duration: 80; easing.type: Easing.InOutCubic }
                    NumberAnimation { target: signin; property: "shiftX"; to: 0; duration: 60; easing.type: Easing.OutCubic }
                }

                RowLayout {
                    anchors { fill: parent; margins: 16 * ui.s }
                    spacing: 14 * ui.s

                    // The avatar: the person's picture when there is one, else the initial,
                    // on a dark circle.
                    Item {
                        Layout.preferredWidth: 52 * ui.s
                        Layout.preferredHeight: 52 * ui.s
                        Layout.alignment: Qt.AlignVCenter
                        Rectangle {
                            anchors.fill: parent
                            radius: width / 2
                            color: ui.dark
                            visible: face.status !== Image.Ready
                            Text {
                                anchors.centerIn: parent
                                text: (ui.who || "?").charAt(0).toUpperCase()
                                color: ui.amber
                                font.family: ui.family
                                font.weight: Font.ExtraBold
                                font.pixelSize: 20 * ui.s
                            }
                        }
                        // The picture is hidden here and drawn inside a rounded, clipping parent.
                        Image {
                            id: face
                            anchors.fill: parent
                            source: ui.face
                            sourceSize: Qt.size(width * 2, height * 2)
                            visible: false
                        }
                        Rectangle {
                            anchors.fill: parent
                            radius: width / 2
                            color: "transparent"
                            clip: true
                            visible: face.status === Image.Ready
                            Image {
                                anchors.fill: parent
                                source: face.status === Image.Ready ? ui.face : ""
                                sourceSize: Qt.size(width * 2, height * 2)
                                fillMode: Image.PreserveAspectCrop
                                smooth: true
                            }
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignVCenter
                        spacing: 4 * ui.s
                        Text {
                            Layout.fillWidth: true
                            text: ui.who
                            color: ui.dark
                            elide: Text.ElideRight
                            font.family: ui.family
                            font.weight: Font.ExtraBold
                            font.pixelSize: 16 * ui.s
                        }
                        // The field: a darker pill on the amber, the hint inside it.
                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 32 * ui.s
                            radius: height / 2
                            color: Qt.rgba(0.086, 0.094, 0.102, field.activeFocus ? 0.2 : 0.12)
                            Behavior on color { ColorAnimation { duration: 150; easing.type: Easing.OutCubic } }
                            TextInput {
                                id: field
                                anchors { fill: parent; leftMargin: 14 * ui.s; rightMargin: 14 * ui.s }
                                verticalAlignment: TextInput.AlignVCenter
                                echoMode: TextInput.Password
                                passwordCharacter: "•"
                                color: ui.dark
                                selectionColor: ui.dark
                                selectedTextColor: ui.amber
                                font.family: ui.family
                                font.weight: Font.DemiBold
                                font.pixelSize: 14 * ui.s
                                clip: true
                                focus: true
                                onAccepted: ui.submit()
                                Keys.onEscapePressed: { text = ""; root.notification = "" }
                                Text {
                                    visible: field.text.length === 0
                                    anchors.verticalCenter: parent.verticalCenter
                                    text: root.notification !== "" ? root.notification : "Type your password"
                                    color: ui.dark
                                    opacity: root.notification !== "" ? 1 : 0.7
                                    font: field.font
                                }
                            }
                        }
                    }

                    // The arrow.
                    Rectangle {
                        Layout.preferredWidth: 40 * ui.s
                        Layout.preferredHeight: 40 * ui.s
                        Layout.alignment: Qt.AlignVCenter
                        radius: width / 2
                        color: goArea.pressed ? "#000000" : ui.dark
                        scale: goArea.pressed ? 0.94 : (goArea.containsMouse ? 1.06 : 1)
                        Behavior on scale { NumberAnimation { duration: 150; easing.type: Easing.OutCubic } }
                        Shape {
                            id: arrow
                            anchors.centerIn: parent
                            width: 22 * ui.s
                            height: 22 * ui.s
                            layer.enabled: true
                            layer.samples: 4
                            readonly property real k: width / 24
                            ShapePath {
                                strokeColor: ui.amber
                                strokeWidth: 2.2 * arrow.k
                                fillColor: "transparent"
                                capStyle: ShapePath.RoundCap
                                joinStyle: ShapePath.RoundJoin
                                scale: Qt.size(arrow.k, arrow.k)
                                PathSvg { path: "M5 12h14M13 6l6 6-6 6" }
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
            }
        }

        // Right half: only the tiles that have something real to say.
        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.preferredWidth: 1
            spacing: 12 * ui.s

            RowLayout {
                Layout.fillWidth: true
                Layout.preferredHeight: ui.rowH
                spacing: 12 * ui.s
                visible: eventTile.visible || netTile.visible

                // The next event, blue.
                Tile {
                    id: eventTile
                    order: 1
                    color: "#5aa9e6"
                    visible: ui.extras !== null && ui.extras.eventTitle !== ""
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.preferredWidth: 1
                    Column {
                        anchors { fill: parent; margins: 16 * ui.s }
                        spacing: 4 * ui.s
                        Text { text: "Next"; color: "#0e2233"; font.family: ui.family; font.weight: Font.ExtraBold; font.pixelSize: 13 * ui.s }
                        Text {
                            visible: ui.extras !== null && ui.extras.eventTime !== ""
                            text: ui.extras ? ui.extras.eventTime : ""
                            color: "#0e2233"
                            font.family: ui.family
                            font.weight: Font.ExtraBold
                            font.letterSpacing: -0.03 * 28 * ui.s
                            font.pixelSize: 28 * ui.s
                        }
                        Text {
                            width: parent.width
                            text: ui.extras ? ui.extras.eventTitle : ""
                            color: "#0e2233"
                            elide: Text.ElideRight
                            font.family: ui.family
                            font.weight: Font.DemiBold
                            font.pixelSize: 13 * ui.s
                        }
                    }
                }

                // Battery and connection.
                Tile {
                    id: netTile
                    order: 2
                    visible: ui.status !== null && (ui.status.hasBattery || ui.status.net !== "")
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.preferredWidth: 1
                    Column {
                        anchors { fill: parent; margins: 16 * ui.s }
                        spacing: 4 * ui.s
                        Text {
                            text: ui.status && ui.status.hasBattery ? "Battery" : "Network"
                            color: ui.muted
                            font.family: ui.family
                            font.weight: Font.ExtraBold
                            font.pixelSize: 13 * ui.s
                        }
                        Text {
                            text: ui.status ? (ui.status.hasBattery ? ui.status.percent + "%" : ui.status.net) : ""
                            color: ui.ink
                            font.family: ui.family
                            font.weight: Font.ExtraBold
                            font.letterSpacing: -0.03 * 40 * ui.s
                            font.pixelSize: (ui.status && ui.status.hasBattery ? 40 : 28) * ui.s
                        }
                        Text {
                            visible: ui.status !== null && ui.status.hasBattery && ui.status.net !== ""
                            text: ui.status ? ui.status.net : ""
                            color: ui.muted
                            font.family: ui.family
                            font.pixelSize: 13 * ui.s
                        }
                    }
                }
            }

            // Now playing.
            Tile {
                order: 3
                visible: ui.extras !== null && ui.extras.playTitle !== ""
                Layout.fillWidth: true
                Layout.preferredHeight: ui.rowH
                RowLayout {
                    anchors { fill: parent; margins: 16 * ui.s }
                    spacing: 12 * ui.s
                    Rectangle {
                        Layout.preferredWidth: 56 * ui.s
                        Layout.preferredHeight: 56 * ui.s
                        radius: 14 * ui.s
                        color: "#f783ac"
                        clip: true
                        Image {
                            anchors.fill: parent
                            source: ui.extras ? ui.extras.playArt : ""
                            sourceSize: Qt.size(width * 2, height * 2)
                            fillMode: Image.PreserveAspectCrop
                            visible: status === Image.Ready
                        }
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2 * ui.s
                        Text {
                            Layout.fillWidth: true
                            text: ui.extras ? ui.extras.playTitle : ""
                            color: ui.ink
                            elide: Text.ElideRight
                            font.family: ui.family
                            font.weight: Font.ExtraBold
                            font.pixelSize: 15 * ui.s
                        }
                        Text {
                            Layout.fillWidth: true
                            text: ui.extras ? ((ui.extras.playing ? "Playing" : "Paused") + (ui.extras.playArtist !== "" ? " · " + ui.extras.playArtist : "")) : ""
                            color: ui.muted
                            elide: Text.ElideRight
                            font.family: ui.family
                            font.pixelSize: 13 * ui.s
                        }
                    }
                    Rectangle {
                        Layout.preferredWidth: 40 * ui.s
                        Layout.preferredHeight: 40 * ui.s
                        radius: width / 2
                        color: ui.ink
                        scale: playArea.pressed ? 0.94 : 1
                        Behavior on scale { NumberAnimation { duration: 150; easing.type: Easing.OutCubic } }
                        Shape {
                            id: playGlyph
                            anchors.centerIn: parent
                            width: 16 * ui.s
                            height: 16 * ui.s
                            layer.enabled: true
                            layer.samples: 4
                            readonly property real k: width / 24
                            ShapePath {
                                strokeColor: "transparent"
                                fillColor: ui.dark
                                scale: Qt.size(playGlyph.k, playGlyph.k)
                                PathSvg { path: ui.extras && ui.extras.playing ? "M6 5h4v14H6zM14 5h4v14h-4z" : "M8 5v14l11-7z" }
                            }
                        }
                        MouseArea {
                            id: playArea
                            anchors.fill: parent
                            onClicked: if (ui.extras) ui.extras.togglePlay()
                        }
                    }
                }
            }

            // Unread, per app.
            Tile {
                order: 5
                visible: ui.extras !== null && ui.extras.unread.length > 0
                Layout.fillWidth: true
                Layout.preferredHeight: ui.rowH
                Column {
                    anchors { fill: parent; margins: 16 * ui.s }
                    spacing: 10 * ui.s
                    Text { text: "Unread"; color: ui.muted; font.family: ui.family; font.weight: Font.ExtraBold; font.pixelSize: 13 * ui.s }
                    Flow {
                        width: parent.width
                        spacing: 16 * ui.s
                        Repeater {
                            model: ui.extras ? ui.extras.unread : []
                            delegate: Row {
                                id: badge
                                required property var modelData
                                spacing: 8 * ui.s
                                Kirigami.Icon {
                                    source: badge.modelData.icon
                                    fallback: "application-x-executable"
                                    width: 28 * ui.s
                                    height: 28 * ui.s
                                    anchors.verticalCenter: parent.verticalCenter
                                }
                                Text {
                                    anchors.verticalCenter: parent.verticalCenter
                                    text: badge.modelData.count
                                    color: ui.ink
                                    font.family: ui.family
                                    font.weight: Font.ExtraBold
                                    font.pixelSize: 22 * ui.s
                                }
                            }
                        }
                    }
                }
            }

            Item { Layout.fillHeight: true }
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
            field.forceActiveFocus();
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

    // A click anywhere on the board puts the focus back in the field; typing starts it.
    MouseArea {
        anchors.fill: parent
        z: -1
        onPressed: field.forceActiveFocus()
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

    Component.onCompleted: {
        authenticator.startAuthenticating();
        field.forceActiveFocus();
    }
}
