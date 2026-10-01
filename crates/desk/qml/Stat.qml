import QtQuick
import QtQuick.Layouts

// One labelled figure.
ColumnLayout {
    id: stat

    required property string label
    required property string value

    spacing: 2

    PlainLabel {
        opacity: 0.6
        font.pixelSize: 11
        text: stat.label
    }

    Mono {
        Layout.fillWidth: true
        font.pixelSize: 18
        text: stat.value
    }
}
