import QtQuick

import desk.bridge

// The header marker, in the terminal client's words and colours.
Row {
    id: badge

    required property int state
    required property bool partial

    readonly property var looks: {
        switch (badge.state) {
        case NestStatus.Live: return { text: "LIVE", color: "#3fa45b" }
        case NestStatus.Attention: return { text: "ATTENTION", color: "#c9a227" }
        case NestStatus.Backfill: return { text: "BACKFILL", color: "#b565c9" }
        case NestStatus.Quarantined: return { text: "QUARANTINED", color: "#d1483f" }
        case NestStatus.Stale: return { text: "STALE", color: "#d1483f" }
        default: return { text: "CONNECTING", color: "#8a8a8a" }
        }
    }

    spacing: 6

    Rectangle {
        anchors.verticalCenter: parent.verticalCenter
        width: 10
        height: 10
        radius: 5
        // A marker that says all is well cannot sit over a panel that could not be fetched.
        color: badge.partial ? "#c9a227" : badge.looks.color
    }

    PlainLabel {
        anchors.verticalCenter: parent.verticalCenter
        font.bold: true
        color: badge.partial ? "#c9a227" : badge.looks.color
        text: badge.partial ? badge.looks.text + " (partial)" : badge.looks.text
    }
}
