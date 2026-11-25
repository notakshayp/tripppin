import QtQuick
import org.kde.notificationmanager as NM
import org.kde.plasma.plasma5support as P5Support

// Counts unread notifications per app from the notification server (this runs inside
// plasmashell, the one process that can see its history) and writes them for the lock screen
// to read: $XDG_RUNTIME_DIR/tripppin-unread.json, one line, [{"icon":"firefox","count":3}].
// Nothing is invented: an app with none unread is not listed, and an empty list is written
// when there is nothing unread.
Item {
    id: unread

    NM.Notifications {
        id: notifications
        showExpired: true
        showDismissed: true
        groupMode: NM.Notifications.GroupDisabled
    }

    Repeater {
        id: rows
        model: notifications
        delegate: Item {
            required property string applicationIconName
            required property bool read
        }
        onCountChanged: debounce.restart()
        onItemAdded: debounce.restart()
        onItemRemoved: debounce.restart()
    }

    P5Support.DataSource {
        id: writer
        engine: "executable"
        onNewData: (source, data) => disconnectSource(source)
    }

    Timer {
        id: debounce
        interval: 500
        onTriggered: {
            var counts = {};
            var order = [];
            for (var i = 0; i < rows.count; i++) {
                var it = rows.itemAt(i);
                if (!it || it.read) continue;
                var icon = it.applicationIconName;
                if (!icon || !/^[A-Za-z0-9._-]+$/.test(icon)) continue;
                if (!counts[icon]) { counts[icon] = 0; order.push(icon); }
                counts[icon] += 1;
            }
            var list = order.map(function (k) { return { icon: k, count: counts[k] }; });
            // The JSON holds only characters that are safe inside single quotes.
            var json = JSON.stringify(list);
            writer.connectSource("sh -c 'mkdir -p \"${XDG_RUNTIME_DIR}\" && printf %s '\\''" + json + "'\\'' > \"${XDG_RUNTIME_DIR}/tripppin-unread.json\"'");
        }
    }

    Component.onCompleted: debounce.restart()
}
