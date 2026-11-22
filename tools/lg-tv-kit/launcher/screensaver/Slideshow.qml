import QtQuick 2.12
import Qt.labs.folderlistmodel 2.12

// Kiro screen saver. kiro-bg.sh bind-mounts this file (RAM only) over LG's ScreenSaver.qml and Clock.qml when
// "Screensaver: On" is chosen in the launcher. It shows the launcher's pictures dimmed, a new one every 30 seconds, with a
// small clock. LG's own screen saver code still decides when it starts and when it closes; this file only draws.
// It has the properties and the signal LG's MainView sets on ScreenSaver.qml and Clock.qml.
Item {
    id: root

    property bool playing: false
    property bool liteMode: false
    property bool isPartial: false
    property string guideString: ""
    property string screenSaverText: ""
    property alias animationTimer: swapTimer
    property alias animation: swapTimer
    signal looped()

    property string front: ""
    property string pending: ""
    property int last: -1

    onPlayingChanged: {
        if (playing) {
            visible = true;
            next();
        } else {
            visible = false;
        }
    }

    FolderListModel {
        id: pics
        folder: "file:///media/developer/kiro/home/app/bg"
        nameFilters: ["*.jpg"]
        showDirs: false
        onCountChanged: {
            if (root.playing && root.front === "" && root.pending === "")
                root.next();
        }
    }

    // LG's MainView puts its normal saver in a small box at a random spot: cover the whole screen from wherever it is
    Item {
        id: stage
        x: -root.x
        y: -root.y
        width: 1920
        height: 1080

        Rectangle {
            anchors.fill: parent
            color: "black"
        }
        Image {
            id: picA
            anchors.fill: parent
            fillMode: Image.PreserveAspectCrop
            asynchronous: true
            cache: false
            sourceSize.width: 1920
            sourceSize.height: 1080
            opacity: root.front === "a" ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: 2000 } }
            onStatusChanged: {
                if (status === Image.Ready && root.pending === "a") {
                    root.front = "a";
                    root.pending = "";
                }
            }
        }
        Image {
            id: picB
            anchors.fill: parent
            fillMode: Image.PreserveAspectCrop
            asynchronous: true
            cache: false
            sourceSize.width: 1920
            sourceSize.height: 1080
            opacity: root.front === "b" ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: 2000 } }
            onStatusChanged: {
                if (status === Image.Ready && root.pending === "b") {
                    root.front = "b";
                    root.pending = "";
                }
            }
        }
        // the dimming
        Rectangle {
            anchors.fill: parent
            color: "black"
            opacity: 0.45
        }
        Text {
            id: clock
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.rightMargin: 90
            anchors.bottomMargin: 70
            color: "white"
            opacity: 0.75
            font.pixelSize: 64
            font.family: "LG Smart UI"
            font.weight: Font.Light
        }
    }

    Timer {
        id: swapTimer
        interval: 30000
        repeat: true
        running: root.playing
        onTriggered: root.next()
    }
    Timer {
        interval: 15000
        repeat: true
        running: root.playing
        triggeredOnStart: true
        onTriggered: clock.text = Qt.formatTime(new Date(), "h:mm AP")
    }

    function next() {
        if (pics.count === 0)
            return;
        var i = Math.floor(Math.random() * pics.count);
        if (pics.count > 1 && i === last)
            i = (i + 1) % pics.count;
        last = i;
        var url = pics.get(i, "fileURL");
        var target = front === "a" ? picB : picA;
        var name = front === "a" ? "b" : "a";
        if (target.source == url && target.status === Image.Ready) {
            front = name;
            pending = "";
        } else {
            pending = name;
            target.source = url;
        }
    }
}
