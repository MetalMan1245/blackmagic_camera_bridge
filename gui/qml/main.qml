import QtQuick
import QtQuick.Controls
import Camlink

ApplicationWindow {
    id: window
    visible: true
    width: 540
    height: 560
    title: "camlink — BMPCC ↔ REAPER"

    // Poll the Rust GuiCore for state changes (status, scan results, logs).
    Timer {
        interval: 250
        running: true
        repeat: true
        triggeredOnStart: true
        onTriggered: Camlink.poll()
    }

    Column {
        anchors.fill: parent
        anchors.margins: 12
        spacing: 10

        GroupBox {
            title: "Status"
            width: parent.width
            Column {
                width: parent.width
                Label { text: "BLE adapter: " + (Camlink.adapterReady ? "ready" : "not initialized") }
                Label { text: "Camera:      " + (Camlink.cameraAddress === "" ? "— not set —" : Camlink.cameraAddress) }
                Label { text: "Connection:  " + Camlink.connectionLabel }
                Label { text: "OSC target:  " + Camlink.oscTarget }
                Label { text: "Recording:   " + Camlink.recordState }
            }
        }

        Row {
            spacing: 8
            Button {
                text: Camlink.scanning ? "Scanning…" : "Scan for cameras"
                enabled: !Camlink.scanning
                onClicked: Camlink.scan()
            }
            ComboBox {
                id: deviceCombo
                model: Camlink.devices
                implicitWidth: 330
                enabled: Camlink.devices.length > 0
                textRole: "name"
            }
        }

        GroupBox {
            title: "Manual camera address (fallback)"
            width: parent.width
            Column {
                width: parent.width
                spacing: 6
                TextField {
                    id: manualMac
                    placeholderText: "AA:BB:CC:DD:EE:FF"
                    text: Camlink.cameraAddress
                    width: parent.width
                }
                Row {
                    spacing: 8
                    Button {
                        text: "Connect selected"
                        enabled: deviceCombo.currentIndex >= 0 && !Camlink.connecting
                        onClicked: Camlink.connectTo(deviceCombo.model[deviceCombo.currentIndex].address)
                    }
                    Button {
                        text: Camlink.connecting ? "Connecting…" : "Connect manual"
                        enabled: !Camlink.connecting
                        onClicked: Camlink.connectTo(manualMac.text)
                    }
                    Button {
                        text: "Disconnect"
                        enabled: Camlink.connected
                        onClicked: Camlink.disconnect()
                    }
                }
            }
        }

        GroupBox {
            title: "Diagnostics (same output as the CLI)"
            width: parent.width
            height: 220
            ScrollView {
                anchors.fill: parent
                TextArea {
                    readOnly: true
                    text: Camlink.logLines.join("\n")
                    font.family: "monospace"
                    wrapMode: TextArea.Wrap
                }
            }
        }
    }
}
