import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ApplicationWindow {
    id: root

    // Set from Rust before the window is created: one entry per nest in `nests.toml`.
    property var nestNames: []
    property var nestUrls: []
    property var nestNotes: []
    // Why `nests.toml` could not be used, when it could not.
    property string configProblem: ""

    // Exposed for the headless tests.
    readonly property alias pages: pages

    function open(name, url, note) {
        nests.append({ name: name, url: url, note: note })
        bar.currentIndex = nests.count - 1
    }

    width: 1180
    height: 760
    visible: true
    title: "nuthatch-desk"

    Component.onCompleted: {
        for (let i = 0; i < root.nestUrls.length; i++)
            nests.append({ name: root.nestNames[i], url: root.nestUrls[i], note: root.nestNotes[i] })
        bar.currentIndex = 0
    }

    ListModel {
        id: nests
    }

    header: ColumnLayout {
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            TabBar {
                id: bar
                Layout.fillWidth: true

                Repeater {
                    model: nests

                    TabButton {
                        id: tab

                        required property int index
                        required property string name

                        width: Math.max(120, label.implicitWidth + 56)

                        // The name comes from a file, so it is shown as plain text like the rest.
                        contentItem: RowLayout {
                            spacing: 6

                            PlainLabel {
                                id: label
                                Layout.fillWidth: true
                                horizontalAlignment: Text.AlignHCenter
                                text: tab.name
                            }

                            ToolButton {
                                text: "×"
                                implicitWidth: 22
                                implicitHeight: 22
                                onClicked: nests.remove(tab.index)
                            }
                        }
                    }
                }
            }

            TextField {
                id: address
                Layout.preferredWidth: 260
                placeholderText: "http://127.0.0.1:8288"
                selectByMouse: true
                onAccepted: add.clicked()
            }

            Button {
                id: add
                Layout.rightMargin: 8
                text: "Open"
                enabled: address.text.trim() !== ""
                onClicked: {
                    root.open(address.text.trim(), address.text.trim(), "")
                    address.clear()
                }
            }
        }

        PlainLabel {
            Layout.fillWidth: true
            Layout.margins: 8
            visible: root.configProblem !== ""
            color: "#d1483f"
            wrapMode: Text.Wrap
            elide: Text.ElideNone
            text: root.configProblem
        }
    }

    StackLayout {
        anchors.fill: parent
        currentIndex: bar.currentIndex

        Repeater {
            id: pages
            model: nests

            NestPage {}
        }
    }

    PlainLabel {
        anchors.centerIn: parent
        visible: nests.count === 0
        opacity: 0.6
        text: "No nest open. Type a nest's URL above and press Open."
    }
}
