import QtQuick
import org.kde.kirigami as Kirigami
import org.kde.plasma.plasma5support as P5Support

// The two optional lines of stage 1 (mock section 01): the next event (bold title, muted time
// line) and the unread count per app (the app's icon and a number). Both come from small
// files other parts of the system write, and both are left out when there is nothing real:
//
//   next event   $XDG_DATA_HOME/tripppin/next-event.json (default ~/.local/share/tripppin/),
//                one line: {"title":"Design review","when":"Tomorrow 10:00 – 11:00","until":1795700000}
//                `until` is when it stops mattering (seconds since the epoch); written by the
//                calendar app.
//   unread       $XDG_RUNTIME_DIR/tripppin-unread.json, one line:
//                [{"icon":"firefox","count":3}]; written by the taskbar's status plasmoid
//                from the notification server (the locker cannot see plasmashell's history).
Column {
    id: extras
    property real s: 1
    property string eventTitle: ""
    property string eventWhen: ""
    property var unread: []

    function parse(text) {
        var lines = (text || "").split("\n");
        var evLine = "", unLine = "";
        for (var n = 0; n < lines.length; n++) {
            if (lines[n].indexOf("E:") === 0) evLine = lines[n].substring(2);
            else if (lines[n].indexOf("U:") === 0) unLine = lines[n].substring(2);
        }
        var title = "", when = "", list = [];
        try {
            var e = JSON.parse(evLine);
            if (e && e.title && (!e.until || e.until * 1000 > Date.now())) {
                title = String(e.title);
                when = e.when ? String(e.when) : "";
            }
        } catch (err) {}
        try {
            var u = JSON.parse(unLine);
            if (Array.isArray(u)) {
                for (var i = 0; i < u.length && list.length < 6; i++)
                    if (u[i] && u[i].icon && u[i].count > 0) list.push({ icon: String(u[i].icon), count: Number(u[i].count) });
            }
        } catch (err2) {}
        eventTitle = title;
        eventWhen = when;
        unread = list;
    }

    P5Support.DataSource {
        engine: "executable"
        connectedSources: ["sh -c 'cat \"${XDG_DATA_HOME:-$HOME/.local/share}/tripppin/next-event.json\" 2>/dev/null | head -n 1 | sed \"s,^,E:,\"; cat \"${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/tripppin-unread.json\" 2>/dev/null | head -n 1 | sed \"s,^,U:,\"'"]
        interval: 5000
        onNewData: (source, data) => extras.parse(data["stdout"])
    }

    Item { width: 1; height: extras.eventTitle !== "" ? 18 * extras.s : 0 }
    Text {
        visible: extras.eventTitle !== ""
        text: extras.eventTitle
        color: "#ffffff"
        font.family: "Inter"
        font.weight: Font.DemiBold
        font.pixelSize: 16 * extras.s
    }
    Text {
        visible: extras.eventTitle !== "" && extras.eventWhen !== ""
        text: extras.eventWhen
        color: "#9aa4ab"
        font.family: "Inter"
        font.pixelSize: 15 * extras.s
    }
    Item { width: 1; height: extras.unread.length > 0 ? 22 * extras.s : 0 }
    Row {
        visible: extras.unread.length > 0
        spacing: 22 * extras.s
        Repeater {
            model: extras.unread
            delegate: Row {
                id: badge
                required property var modelData
                spacing: 8 * extras.s
                Kirigami.Icon {
                    source: badge.modelData.icon
                    fallback: "application-x-executable"
                    width: 22 * extras.s
                    height: 22 * extras.s
                    anchors.verticalCenter: parent.verticalCenter
                }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: badge.modelData.count
                    color: "#ffffff"
                    font.family: "Inter"
                    font.weight: Font.Light
                    font.pixelSize: 18 * extras.s
                }
            }
        }
    }
}
