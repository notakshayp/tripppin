import QtQuick
import org.kde.plasma.plasma5support as P5Support

// The person's full name from the passwd entry (the part of the name field before the first
// comma). Nothing is emitted when there is none, and the lock screen shows the login.
Item {
    id: lookup
    signal result(string name)

    P5Support.DataSource {
        engine: "executable"
        connectedSources: ["sh -c 'getent passwd \"$(id -un)\"'"]
        onNewData: (source, data) => {
            var line = (data["stdout"] || "").split("\n")[0];
            var gecos = line.split(":")[4] || "";
            var name = gecos.split(",")[0].trim();
            if (name !== "") lookup.result(name);
            disconnectSource(source);
        }
    }
}
