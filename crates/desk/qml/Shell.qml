pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The whole of the window's contents: a tab per nest, and a field to open another.
Item {
    id: shell

    required property var nestNames
    required property var nestUrls
    required property var nestSsh
    required property string configProblem

    // Exposed for the headless tests.
    readonly property alias pages: pages

    function open(name, url, ssh) {
        nests.append({ name: name, url: url, ssh: ssh })
        bar.currentIndex = nests.count - 1
    }

    Component.onCompleted: {
        for (let i = 0; i < shell.nestUrls.length; i++)
            nests.append({ name: shell.nestNames[i], url: shell.nestUrls[i], ssh: shell.nestSsh[i] })
        bar.currentIndex = 0
    }

    ListModel {
        id: nests
    }

    Rectangle {
        anchors.fill: parent
        color: palette.window
    }

    ColumnLayout {
        anchors.fill: parent
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
                    shell.open(address.text.trim(), address.text.trim(), "")
                    address.clear()
                }
            }
        }

        PlainLabel {
            Layout.fillWidth: true
            Layout.margins: 8
            visible: shell.configProblem !== ""
            color: "#d1483f"
            wrapMode: Text.Wrap
            elide: Text.ElideNone
            text: shell.configProblem
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: bar.currentIndex

            Repeater {
                id: pages
                model: nests

                NestPage {}
            }
        }
    }

    PlainLabel {
        anchors.centerIn: parent
        visible: nests.count === 0
        opacity: 0.6
        text: "No nest open. Type a nest's URL above and press Open."
    }
}
