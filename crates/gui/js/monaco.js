import * as monaco from 'monaco-editor';

window.initMonaco = function(code) {
    const editorElement = document.getElementById("editor");
    if (!editorElement) {
        console.error("Editor element not found");
        return;
    }
    
    const myEditor = monaco.editor.create(editorElement, {
        value: code,
        language: "yaml",
        automaticLayout: true,
    });

    window.monacoEditor = myEditor;
};

window.updateMonaco = function(code) {
    if (window.monacoEditor) {
        window.monacoEditor.setValue(code);
    } else {
        console.error("Monaco editor not initialized");
    }
};

window.getMonacoValue = function() {
    if (window.monacoEditor) {
        var code = window.monacoEditor.getValue()
        return code
    } else {
        console.error("Monaco editor not initialized");
    }
}