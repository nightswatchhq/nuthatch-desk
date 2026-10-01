pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls

import desk.bridge

// A result in a scrolling grid. Clicking a header sorts by it; clicking again reverses.
Item {
    id: grid

    required property ResultModel model
    property bool sortable: true

    // A column is as wide as its widest cell on screen, and never narrower than its name.
    FontMetrics {
        id: heading
        font.bold: true
    }

    HorizontalHeaderView {
        id: header
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        syncView: table
        clip: true

        delegate: Rectangle {
            id: title

            required property int column
            required property string display

            implicitWidth: Math.max(80, name.implicitWidth + 28)
            implicitHeight: 28
            color: palette.button
            border.color: palette.mid

            PlainLabel {
                id: name
                anchors.fill: parent
                anchors.leftMargin: 8
                anchors.rightMargin: 8
                verticalAlignment: Text.AlignVCenter
                font.bold: true
                text: {
                    if (grid.model.sortColumn !== title.column)
                        return title.display
                    return title.display + (grid.model.sortDescending ? "  ▼" : "  ▲")
                }
            }

            MouseArea {
                anchors.fill: parent
                enabled: grid.sortable
                onClicked: grid.model.sortBy(title.column)
            }
        }
    }

    TableView {
        id: table
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: header.bottom
        anchors.bottom: parent.bottom
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        model: grid.model

        ScrollBar.vertical: ScrollBar {}
        ScrollBar.horizontal: ScrollBar {}

        delegate: Rectangle {
            id: cell

            required property int row
            required property int column
            required property string display
            required property bool isNull

            implicitWidth: Math.min(600, Math.max(heading.advanceWidth(grid.model.columnNames[cell.column]) + 44, value.implicitWidth + 16))
            implicitHeight: 24
            color: cell.row % 2 === 0 ? palette.base : palette.alternateBase

            Mono {
                id: value
                anchors.fill: parent
                anchors.leftMargin: 8
                anchors.rightMargin: 8
                verticalAlignment: Text.AlignVCenter
                opacity: cell.isNull ? 0.4 : 1
                text: cell.isNull ? "NULL" : cell.display
            }
        }
    }
}
