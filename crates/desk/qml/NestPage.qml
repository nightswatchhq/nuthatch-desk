pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import desk.bridge

// Everything shown for one nest. Each page has its own objects, so one nest stalling, restarting
// or going away changes nothing on another page.
Item {
    id: page

    required property string url
    // The ssh host the nest is reached through, or empty. `url` is then as seen from that host.
    required property string ssh

    // Exposed for the headless tests.
    readonly property alias status: status
    readonly property alias tables: tables
    readonly property alias query: query
    readonly property alias feed: feed
    readonly property alias results: results
    property alias section: sections.currentIndex

    function grouped(value) {
        return value < 0 ? "–" : value.toLocaleString(Qt.locale("en_US"), "f", 0)
    }

    function copy(text) {
        clipboard.text = text
        clipboard.selectAll()
        clipboard.copy()
        clipboard.text = ""
    }

    Component.onCompleted: {
        if (page.ssh === "")
            status.connectTo(page.url)
        else
            status.connectVia(page.url, page.ssh)
    }

    NestStatus {
        id: status
        onRestartSeen: restarted.restart()
    }

    TableList {
        id: tables
        url: status.url
    }

    SqlQuery {
        id: query
        url: status.url
    }

    // Qt registers QAbstractListModel with QML and not QAbstractTableModel, so qmllint cannot
    // follow ResultModel to its base. The engine can, and the headless tests load this file.
    // qmllint disable unresolved-type
    ResultModel {
        id: feed
        resultKey: tables.feedKey
    }

    ResultModel {
        id: results
        resultKey: query.resultKey
    }
    // qmllint enable unresolved-type

    // The clipboard is reached through a text item: QML has no clipboard type of its own.
    TextEdit {
        id: clipboard
        visible: false
        textFormat: TextEdit.PlainText
    }

    // Keeps the restart notice up for ten minutes, as the terminal client does.
    Timer {
        id: restarted
        interval: 600000
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 12
        spacing: 10

        RowLayout {
            Layout.fillWidth: true
            spacing: 14

            StateBadge {
                nestState: status.state
                partial: status.partial
            }

            PlainLabel {
                font.bold: true
                font.pixelSize: 16
                text: status.nestName !== "" ? status.nestName : "nest"
            }

            PlainLabel {
                visible: status.chain !== ""
                text: status.chain
            }

            PlainLabel {
                visible: status.version !== ""
                opacity: 0.6
                text: "nuthatch " + status.version
            }

            // The nest's own address. Behind ssh that is as seen from the host, and what is
            // polled is the forward's end on this machine.
            Mono {
                Layout.fillWidth: true
                opacity: 0.6
                text: page.ssh === "" ? status.url : page.url + " through ssh " + page.ssh
            }

            PlainLabel {
                visible: restarted.running
                color: "#c9a227"
                text: "restarted"
            }

            Button {
                text: "Refresh"
                onClicked: status.refresh()
            }
        }

        PlainLabel {
            Layout.fillWidth: true
            visible: status.insecure
            color: "#c9a227"
            wrapMode: Text.Wrap
            elide: Text.ElideNone
            text: "Plain HTTP to a remote host: what this page shows can be read and altered in transit. Reach the nest through an ssh forward or TLS."
        }

        PlainLabel {
            Layout.fillWidth: true
            visible: status.problems !== ""
            color: "#d1483f"
            wrapMode: Text.Wrap
            elide: Text.ElideNone
            text: status.problems
        }

        TabBar {
            id: sections
            Layout.fillWidth: true

            TabButton { text: "Overview" }
            TabButton { text: "Tables" }
            TabButton { text: "SQL" }
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: sections.currentIndex

            // Overview: health, sync and the charts.
            ColumnLayout {
                spacing: 12

                GridLayout {
                    Layout.fillWidth: true
                    columns: 4
                    columnSpacing: 24
                    rowSpacing: 12

                    Stat { Layout.fillWidth: true; label: "TIP"; value: page.grouped(status.tip) }
                    Stat { Layout.fillWidth: true; label: "INDEXED"; value: page.grouped(status.indexed) }
                    Stat { Layout.fillWidth: true; label: "LAG, BLOCKS"; value: page.grouped(status.lagBlocks) }
                    Stat { Layout.fillWidth: true; label: "HOT ROWS"; value: page.grouped(status.hotRows) }
                    Stat {
                        Layout.fillWidth: true
                        label: "SEALED THROUGH"
                        value: status.sealed > 0 ? page.grouped(status.sealed) : "nothing sealed"
                    }
                    Stat {
                        Layout.fillWidth: true
                        label: "SEAL GAP, BLOCKS"
                        value: status.sealed > 0 ? page.grouped(status.sealGap) : "–"
                    }
                    Stat {
                        Layout.fillWidth: true
                        label: "POLL INTERVAL"
                        value: status.pollIntervalSecs > 0 ? status.pollIntervalSecs + " s" : "–"
                    }
                    Stat { Layout.fillWidth: true; label: "ROUND TRIP"; value: status.refreshMs + " ms" }
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 12

                    PlainLabel {
                        opacity: 0.6
                        text: "SYNC"
                    }

                    ProgressBar {
                        Layout.fillWidth: true
                        value: status.syncFraction
                    }

                    Mono {
                        text: status.syncLabel
                    }
                }

                GridLayout {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    columns: 3
                    columnSpacing: 10
                    rowSpacing: 10

                    Chart {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        url: status.url
                        metric: "lag"
                        title: "Lag"
                        unit: " blocks"
                    }
                    Chart {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        url: status.url
                        metric: "rate:nuthatch_rows_decoded_total"
                        title: "Rows decoded"
                        unit: " /s"
                        decimals: 1
                    }
                    Chart {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        url: status.url
                        metric: "rate:nuthatch_rpc_requests_total"
                        title: "RPC requests"
                        unit: " /s"
                        decimals: 2
                    }
                    Chart {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        url: status.url
                        metric: "cpu"
                        title: "CPU"
                        unit: " %"
                        decimals: 1
                    }
                    Chart {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        url: status.url
                        metric: "gauge:nuthatch_rss_bytes"
                        title: "Memory"
                        unit: " MiB"
                        factor: 1 / 1048576
                    }
                    Chart {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        url: status.url
                        metric: "refresh_ms"
                        title: "Round trip"
                        unit: " ms"
                    }
                }
            }

            // Tables: the catalogue, and the newest rows of the selected table.
            SplitView {
                orientation: Qt.Horizontal

                ListView {
                    id: catalogue
                    SplitView.preferredWidth: 260
                    SplitView.minimumWidth: 160
                    clip: true
                    model: tables
                    boundsBehavior: Flickable.StopAtBounds
                    ScrollBar.vertical: ScrollBar {}

                    section.property: "group"
                    section.delegate: PlainLabel {
                        required property string section

                        width: catalogue.width
                        topPadding: 8
                        bottomPadding: 4
                        leftPadding: 8
                        font.bold: true
                        opacity: 0.6
                        text: section
                    }

                    delegate: ItemDelegate {
                        id: entry

                        required property int index
                        required property string name

                        width: catalogue.width
                        highlighted: entry.index === tables.selected
                        onClicked: tables.select(entry.index)

                        contentItem: Mono {
                            leftPadding: 8
                            text: entry.name
                        }
                    }
                }

                ColumnLayout {
                    SplitView.fillWidth: true
                    spacing: 8

                    RowLayout {
                        Layout.fillWidth: true
                        Layout.leftMargin: 10
                        spacing: 14

                        Mono {
                            font.bold: true
                            text: tables.selectedTable !== "" ? tables.selectedTable : "no table"
                        }

                        PlainLabel {
                            visible: tables.selectedRows >= 0
                            text: page.grouped(tables.selectedRows) + " rows"
                        }

                        PlainLabel {
                            visible: tables.selectedLatestBlock >= 0
                            text: "latest block " + page.grouped(tables.selectedLatestBlock)
                        }

                        PlainLabel {
                            Layout.fillWidth: true
                            color: "#c9a227"
                            text: tables.sqlOpen ? tables.feedNotice : "this nest answers declared queries only, so there is no preview"
                        }

                        Button {
                            text: "Query"
                            enabled: tables.sqlOpen && tables.selectedTable !== ""
                            onClicked: {
                                editor.text = "SELECT * FROM \"" + tables.selectedTable + "\"\nORDER BY block_number DESC\nLIMIT 100"
                                sections.currentIndex = 2
                            }
                        }
                    }

                    DataGrid {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        Layout.leftMargin: 10
                        model: feed
                    }
                }
            }

            // SQL: a statement, and what it returned.
            ColumnLayout {
                spacing: 8

                ScrollView {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 130

                    TextArea {
                        id: editor
                        enabled: tables.sqlOpen
                        font.family: Qt.platform.os === "osx" ? "Menlo" : Qt.platform.os === "windows" ? "Consolas" : "monospace"
                        textFormat: TextEdit.PlainText
                        wrapMode: TextEdit.Wrap
                        selectByMouse: true
                        placeholderText: tables.sqlOpen ? "SELECT ... (Ctrl+Return runs it)" : "This nest answers declared queries only."
                    }
                }

                Shortcut {
                    sequence: "Ctrl+Return"
                    // Only the page in front: two tabs both claiming the keys would get neither.
                    enabled: page.visible && sections.currentIndex === 2 && tables.sqlOpen
                    onActivated: query.run(editor.text)
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10

                    Button {
                        text: "Run"
                        enabled: tables.sqlOpen && !query.running
                        onClicked: query.run(editor.text)
                    }

                    Button {
                        text: "Cancel"
                        enabled: query.running
                        onClicked: query.cancel()
                    }

                    Button {
                        text: "Copy CSV"
                        enabled: results.rows > 0
                        onClicked: page.copy(results.csv())
                    }

                    Button {
                        text: "Unsorted"
                        visible: results.sortColumn >= 0
                        onClicked: results.clearSort()
                    }

                    PlainLabel {
                        Layout.fillWidth: true
                        text: {
                            if (query.running)
                                return "running"
                            if (query.resultKey === 0)
                                return ""
                            return page.grouped(query.rowCount) + " rows in " + query.elapsedMs + " ms"
                        }
                    }

                    PlainLabel {
                        color: "#c9a227"
                        text: query.notice
                    }
                }

                // The nest's own wording, line breaks and all.
                Mono {
                    Layout.fillWidth: true
                    visible: query.error !== ""
                    color: "#d1483f"
                    wrapMode: Text.Wrap
                    elide: Text.ElideNone
                    text: query.error
                }

                DataGrid {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    model: results
                }
            }
        }
    }
}
