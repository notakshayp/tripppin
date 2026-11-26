import QtQuick
import org.kde.plasma.plasma5support as P5Support

// The optional tiles of the lock board (Bento mock 01) as data: the next event, what is
// playing, and the unread count per app. All of it comes from small files or programs other
// parts of the system provide, and a tile is left out when there is nothing real:
//
//   next event   $XDG_DATA_HOME/tripppin/next-event.json (default ~/.local/share/tripppin/),
//                one line: {"title":"Design review","when":"Tomorrow 10:00 - 11:00","until":1795700000}
//                `until` is when it stops mattering (seconds since the epoch); written by the
//                calendar app. The tile's big number is the first time in `when`.
//   now playing  MPRIS, through `playerctl metadata` (title, artist, state, cover when it is a
//                local file).
//   unread       $XDG_RUNTIME_DIR/tripppin-unread.json, one line:
//                [{"icon":"firefox","count":3}]; written by the rail's status plasmoid from the
//                notification server (the locker cannot see plasmashell's history).
Item {
    id: extras
    property string eventTitle: ""
    property string eventWhen: ""
    property string eventTime: ""
    property var unread: []
    property string playTitle: ""
    property string playArtist: ""
    property bool playing: false
    property string playArt: ""

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
        var t = /\d{1,2}:\d{2}/.exec(when);
        eventTime = t ? t[0] : "";
        unread = list;
    }

    function parsePlayer(text) {
        var parts = (text || "").split("\n")[0].split("<|>");
        if (parts.length < 4 || parts[1] === "" || (parts[0] !== "Playing" && parts[0] !== "Paused")) {
            playTitle = ""; playArtist = ""; playing = false; playArt = "";
            return;
        }
        playing = parts[0] === "Playing";
        playTitle = parts[1];
        playArtist = parts[2];
        playArt = parts[3].indexOf("file://") === 0 ? parts[3] : "";
    }

    P5Support.DataSource {
        engine: "executable"
        connectedSources: [
            "sh -c 'cat \"${XDG_DATA_HOME:-$HOME/.local/share}/tripppin/next-event.json\" 2>/dev/null | head -n 1 | sed \"s,^,E:,\"; cat \"${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/tripppin-unread.json\" 2>/dev/null | head -n 1 | sed \"s,^,U:,\"'",
            "playerctl metadata --format '{{status}}<|>{{title}}<|>{{artist}}<|>{{mpris:artUrl}}'"
        ]
        interval: 3000
        onNewData: (source, data) => {
            if (source.indexOf("playerctl") === 0) extras.parsePlayer(data["stdout"]);
            else extras.parse(data["stdout"]);
        }
    }

    // Play and pause from the tile.
    function togglePlay() { control.connectSource("playerctl play-pause") }
    P5Support.DataSource {
        id: control
        engine: "executable"
        onNewData: (source, data) => disconnectSource(source)
    }
}
