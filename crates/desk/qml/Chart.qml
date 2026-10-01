import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Shapes

import desk.bridge

// One metric over the last hour, drawn with QML's own shapes. Qt Charts and Qt Graphs are GPL for
// open-source use, which this project's licence cannot take.
Frame {
    id: chart

    required property string url
    required property string metric
    required property string title
    property string unit: ""
    property int decimals: 0
    // Multiplies the metric before it is worded, for bytes shown as MiB. Not `scale`, which is
    // the item's own and would shrink the chart.
    property real factor: 1

    function figure(value) {
        return (value * chart.factor).toLocaleString(Qt.locale("en_US"), "f", chart.decimals) + chart.unit
    }

    // Exposed for the headless tests.
    readonly property alias series: series

    MetricSeries {
        id: series
        url: chart.url
        metric: chart.metric
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 4

        RowLayout {
            Layout.fillWidth: true

            PlainLabel {
                Layout.fillWidth: true
                font.bold: true
                text: chart.title
            }

            Mono {
                text: series.count > 0 ? chart.figure(series.latest) : (series.known ? "waiting" : "unknown metric")
            }
        }

        Item {
            id: plot
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.minimumHeight: 60

            Rectangle {
                anchors.bottom: parent.bottom
                width: parent.width
                height: 1
                color: palette.mid
            }

            Shape {
                anchors.fill: parent

                ShapePath {
                    strokeColor: "#5b8bd0"
                    strokeWidth: 1.5
                    fillColor: "transparent"
                    capStyle: ShapePath.RoundCap
                    joinStyle: ShapePath.RoundJoin

                    PathPolyline {
                        // `revision` is read only so the line is drawn again when the points change.
                        path: series.revision >= 0 ? series.polyline(plot.width, plot.height) : []
                    }
                }
            }
        }

        PlainLabel {
            opacity: 0.6
            font.pixelSize: 11
            text: series.count > 0 ? "peak " + chart.figure(series.peak) + " · " + series.count + " points" : ""
        }
    }
}
