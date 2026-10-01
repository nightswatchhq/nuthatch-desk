import QtQuick
import QtQuick.Controls

ApplicationWindow {
    id: root

    // Set from Rust before the window is created: one entry per nest in `nests.toml`.
    property var nestNames: []
    property var nestUrls: []
    // The ssh host each is reached through, or empty.
    property var nestSsh: []
    // Why `nests.toml` could not be used, when it could not.
    property string configProblem: ""

    // Exposed for the headless tests.
    readonly property alias shell: shell
    readonly property alias pages: shell.pages

    width: 1180
    height: 760
    visible: true
    title: "nuthatch-desk"

    Shell {
        id: shell
        anchors.fill: parent
        nestNames: root.nestNames
        nestUrls: root.nestUrls
        nestSsh: root.nestSsh
        configProblem: root.configProblem
    }
}
