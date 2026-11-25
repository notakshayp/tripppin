import QtQuick
import org.kde.plasma.plasma5support as P5Support
import org.kde.plasma.private.battery

// Top-right of the lock screen (mock section 01): "Wi-Fi   82%", 13 px, white. Wi-Fi shows
// while connected over Wi-Fi ("Wired" over a cable), the percentage only with a battery.
Row {
    id: status
    property real s: 1
    property string net: ""
    spacing: 16 * s

    BatteryControlModel { id: battery }

    P5Support.DataSource {
        engine: "executable"
        connectedSources: ["nmcli -t -f TYPE,STATE device"]
        interval: 15000
        onNewData: (source, data) => {
            var out = data["stdout"] || "";
            if (out.indexOf("wifi:connected\n") >= 0 || /wifi:connected$/m.test(out))
                status.net = "Wi-Fi";
            else if (out.indexOf("ethernet:connected") >= 0)
                status.net = "Wired";
            else
                status.net = "";
        }
    }

    Text {
        visible: status.net !== ""
        text: status.net
        color: "#ffffff"
        font.family: "Inter"
        font.pixelSize: 13 * status.s
    }
    Text {
        visible: battery.hasInternalBatteries
        text: battery.percent + "%"
        color: "#ffffff"
        font.family: "Inter"
        font.pixelSize: 13 * status.s
    }
}
