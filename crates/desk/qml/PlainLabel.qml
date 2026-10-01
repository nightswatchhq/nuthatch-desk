import QtQuick
import QtQuick.Controls

// Every string a nest supplies is shown through this. A `Text` left on its default format would
// render markup from a hostile nest, and fetch an image for it.
Label {
    textFormat: Text.PlainText
    elide: Text.ElideRight
}
