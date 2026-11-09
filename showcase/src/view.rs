//! The windows, in Slint's own language. Each element sits at a fixed place, so a test can click it.
//!
//! The library window, 400 by 300: the search box on top, a row of 20 pixels per document from y 30, and the buttons at y 240. A
//! row is picked from x 0 to 240, shows its history from 240 to 320, and opens from 320 to 400.
//!
//! The editor window, 400 by 300: the title box on top, the body from y 30, the buttons at y 170, and the question at y 250.

#![allow(missing_docs, missing_debug_implementations, reason = "`slint!` makes the views' types public, with no docs or `Debug`")]

slint::slint! {
    export struct DocRow {
        id: int,
        title: string,
        selected: bool,
    }

    export struct RevisionRow {
        revision: int,
        title: string,
    }

    component Button inherits TouchArea {
        in property <string> label;
        height: 20px;
        Text {
            text: root.label;
        }
    }

    export component LibraryWindow inherits Window {
        width: 400px;
        height: 300px;

        in property <[DocRow]> rows;
        in property <string> status;
        in property <int> page-size;
        in property <bool> word-count;
        in property <bool> history-shown;
        in property <string> history-title;
        in property <[RevisionRow]> revisions;

        callback typed(string);
        callback select(int);
        callback show-history(int);
        callback open-document(int);
        callback previous-page();
        callback next-page();
        callback refresh();
        callback page-size-changed(int);
        callback word-count-changed(bool);

        out property <string> query-shown: search.text;
        search := TextInput {
            x: 0;
            y: 0;
            width: 400px;
            height: 20px;
            edited => {
                root.typed(self.text);
            }
        }

        for row[index] in root.rows: Rectangle {
            x: 0;
            y: 30px + index * 20px;
            width: 400px;
            height: 20px;
            background: row.selected ? #dde : transparent;
            Button {
                x: 0;
                width: 240px;
                label: row.title;
                clicked => {
                    root.select(row.id);
                }
            }
            Button {
                x: 240px;
                width: 80px;
                label: "History";
                clicked => {
                    root.show-history(row.id);
                }
            }
            Button {
                x: 320px;
                width: 80px;
                label: "Open";
                clicked => {
                    root.open-document(row.id);
                }
            }
        }

        Button {
            x: 0;
            y: 240px;
            width: 80px;
            label: "Previous";
            clicked => {
                root.previous-page();
            }
        }
        Button {
            x: 80px;
            y: 240px;
            width: 80px;
            label: "Next";
            clicked => {
                root.next-page();
            }
        }
        Button {
            x: 160px;
            y: 240px;
            width: 80px;
            label: "Refresh";
            clicked => {
                root.refresh();
            }
        }
        Button {
            x: 240px;
            y: 240px;
            width: 40px;
            label: "-";
            clicked => {
                root.page-size-changed(root.page-size - 1);
            }
        }
        Button {
            x: 280px;
            y: 240px;
            width: 40px;
            label: "+";
            clicked => {
                root.page-size-changed(root.page-size + 1);
            }
        }
        Button {
            x: 320px;
            y: 240px;
            width: 80px;
            label: root.word-count ? "Words: on" : "Words: off";
            clicked => {
                root.word-count-changed(!root.word-count);
            }
        }
        Text {
            x: 0;
            y: 270px;
            text: root.status;
        }

        callback back();
        if root.history-shown: Rectangle {
            x: 0;
            y: 0;
            width: 400px;
            height: 300px;
            background: white;
            TouchArea {}
            Button {
                x: 0;
                y: 0;
                width: 100px;
                label: "Back";
                clicked => {
                    root.back();
                }
            }
            Text {
                x: 100px;
                y: 0;
                text: root.history-title;
            }
            for revision[index] in root.revisions: Text {
                x: 0;
                y: 30px + index * 20px;
                text: "Revision " + revision.revision + ": " + revision.title;
            }
        }
    }

    export component EditorWindow inherits Window {
        width: 400px;
        height: 300px;

        // A text box stops following its property once the user types, so each change is copied back. A refused edit leaves
        // the property as it was, so its pulse copies it back too.
        in property <string> doc-title;
        in property <int> title-refused;
        title: "Editing " + root.doc-title;
        in property <string> body;
        callback title-edited(string);
        callback body-edited(string);
        out property <string> title-shown: title-input.text;
        out property <string> body-shown: body-input.text;
        changed doc-title => {
            title-input.text = doc-title;
        }
        changed title-refused => {
            title-input.text = doc-title;
        }
        changed body => {
            body-input.text = body;
        }
        title-input := TextInput {
            x: 0;
            y: 0;
            width: 400px;
            height: 20px;
            edited => {
                root.title-edited(self.text);
            }
        }
        body-input := TextInput {
            x: 0;
            y: 30px;
            width: 400px;
            height: 100px;
            single-line: false;
            edited => {
                root.body-edited(self.text);
            }
        }

        in property <string> status;
        in property <string> problem;
        in property <bool> word-count-shown;
        in property <int> words;
        Text {
            x: 0;
            y: 140px;
            text: root.status + " " + root.problem;
        }
        if root.word-count-shown: Text {
            x: 300px;
            y: 140px;
            text: root.words + " words";
        }

        callback save();
        callback export();
        callback stop-export();
        callback delete();
        Button {
            x: 0;
            y: 170px;
            width: 80px;
            label: "Save";
            clicked => {
                root.save();
            }
        }
        Button {
            x: 80px;
            y: 170px;
            width: 80px;
            label: "Export";
            clicked => {
                root.export();
            }
        }
        Button {
            x: 160px;
            y: 170px;
            width: 80px;
            label: "Stop";
            clicked => {
                root.stop-export();
            }
        }
        Button {
            x: 240px;
            y: 170px;
            width: 80px;
            label: "Delete";
            clicked => {
                root.delete();
            }
        }

        in property <bool> exporting;
        in property <int> export-done;
        in property <int> export-of;
        if root.exporting: Text {
            x: 0;
            y: 200px;
            text: "Exporting " + root.export-done + " of " + root.export-of;
        }

        // A toast shows each time its pulse moves. The window's view counts the pulse from when it attached, so a window opened
        // on a document that toasted before doesn't show that toast.
        in property <string> toast-text;
        in property <int> toast;
        out property <int> toasts-shown;
        out property <string> toast-showing;
        changed toast => {
            toasts-shown += 1;
            toast-showing = toast-text;
        }

        // "" for none, "delete" or "close".
        in property <string> asking;
        callback answer(int);
        if root.asking != "": Rectangle {
            x: 0;
            y: 230px;
            width: 400px;
            height: 70px;
            background: #eee;
            Text {
                x: 0;
                y: 0;
                text: root.asking == "delete" ? "Delete this document?" : "Save the edits before closing?";
            }
            Button {
                x: 0;
                y: 20px;
                width: 130px;
                label: root.asking == "delete" ? "Delete" : "Save";
                clicked => {
                    root.answer(0);
                }
            }
            Button {
                x: 130px;
                y: 20px;
                width: 130px;
                label: "Discard";
                clicked => {
                    root.answer(1);
                }
            }
            Button {
                x: 260px;
                y: 20px;
                width: 140px;
                label: "Cancel";
                clicked => {
                    root.answer(2);
                }
            }
        }
    }
}
