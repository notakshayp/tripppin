import QtQuick
import org.kde.plasma.plasma5support as P5Support
import org.kde.plasma.private.battery

// The lock board's status data (Bento mock 01): how the computer is connected and the battery
// level. Data only; LockScreenUi.qml draws the tile. Wi-Fi shows while connected over Wi-Fi
// ("Wired" over a cable), the battery only with one. Nothing is invented.
Item {
    id: status
    property string net: ""
    readonly property bool hasBattery: battery.hasInternalBatteries
    readonly property int percent: battery.percent

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
}
