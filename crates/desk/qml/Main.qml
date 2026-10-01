import QtQuick
import QtQuick.Controls

import desk.bridge

ApplicationWindow {
    id: root

    width: 960
    height: 600
    visible: true
    title: "nuthatch-desk"

    NestStatus {
        id: status
    }

    Label {
        anchors.centerIn: parent
        textFormat: Text.PlainText
        text: status.url
    }
}
