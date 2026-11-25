import QtQuick
import org.kde.kirigami as Kirigami
import org.kde.notificationmanager as NM

// Unread notifications per app: the app's own icon and the count, 18 px light type (mock
// section 01). The numbers are the notification server's: nothing is shown for an app with
// none, and the whole row is hidden when there is nothing unread.
Item {
    id: badges
    property real s: 1
    property int total: counts.count > 0 ? 1 : 0
    implicitWidth: row.implicitWidth
    implicitHeight: counts.count > 0 ? row.implicitHeight + 22 * s : 0
    width: implicitWidth
    height: implicitHeight

    NM.Notifications {
        id: notifications
        showExpired: true
        showDismissed: true
        groupMode: NM.Notifications.GroupDisabled
    }

    ListModel { id: counts }

    Repeater {
        id: source
        model: notifications
        delegate: Item {
            required property string applicationName
            required property string applicationIconName
            required property bool read
        }
        onCountChanged: recount.restart()
        onItemAdded: recount.restart()
        onItemRemoved: recount.restart()
    }

    Timer {
        id: recount
        interval: 200
        onTriggered: {
            var order = [];
            var byName = {};
            for (var i = 0; i < source.count; i++) {
                var it = source.itemAt(i);
                if (!it || it.read) continue;
                var key = it.applicationName;
                if (!byName[key]) {
                    byName[key] = { icon: it.applicationIconName, name: key, n: 0 };
                    order.push(key);
                }
                byName[key].n += 1;
            }
            counts.clear();
            for (var j = 0; j < order.length && j < 6; j++) counts.append(byName[order[j]]);
        }
    }

    Row {
        id: row
        y: 22 * badges.s
        spacing: 22 * badges.s
        opacity: counts.count > 0 ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: 200; easing.type: Easing.OutCubic } }
        Repeater {
            model: counts
            delegate: Row {
                id: badge
                required property string icon
                required property string name
                required property int n
                spacing: 8 * badges.s
                Kirigami.Icon {
                    source: badge.icon.length > 0 ? badge.icon : "dialog-information"
                    width: 22 * badges.s
                    height: 22 * badges.s
                    anchors.verticalCenter: parent.verticalCenter
                }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: badge.n
                    color: "#ffffff"
                    font.family: "Inter"
                    font.weight: Font.Light
                    font.pixelSize: 18 * badges.s
                }
            }
        }
    }

    Component.onCompleted: recount.restart()
}
