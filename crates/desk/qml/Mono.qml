import QtQuick

// A label in the platform's fixed-width face. "monospace" alone is an alias Qt has to go looking
// for on macOS and Windows, at a cost it complains about on start.
PlainLabel {
    font.family: Qt.platform.os === "osx" ? "Menlo" : Qt.platform.os === "windows" ? "Consolas" : "monospace"
}
