"""Full-screen Textual UI. Optional dependency: pip install textual."""
import json
import shutil
from pathlib import Path
from textual.app import App, ComposeResult
from textual.containers import VerticalScroll, Horizontal, Vertical
from textual.widgets import Header, Footer, Static, TextArea, Markdown, Button
from textual.binding import Binding
from textual import work
from aam_tui import event_label, token_summary

COLORS = {'claude': '#D78B65', 'codex': '#35B9AD'}

class PromptEditor(TextArea):
    """Enter submits; Ctrl+N inserts a newline (works in standard terminals)."""
    BINDINGS = [
        Binding('enter', 'submit_prompt', 'Send', priority=True, show=False),
        Binding('ctrl+n', 'insert_newline', 'New line', priority=True, show=False),
    ]

    def action_submit_prompt(self):
        self.app.action_send()

    def action_insert_newline(self):
        self.insert('\n')


class AgentApp(App):
    TITLE = 'AI Agent Manager'
    BINDINGS = [
        Binding('enter', 'send', 'Send', show=True),
        Binding('ctrl+o', 'toggle_steps', 'Steps', show=True),
        Binding('ctrl+d', 'diff', 'Diff', show=True),
        Binding('ctrl+t', 'switch', 'Agent', show=True),
        Binding('ctrl+q', 'quit', 'Quit', show=True),
    ]
    CSS = """
    Screen { background: #14171c; color: #dce2ea; }
    Header { background: #14171c; color: #9ba8b9; height: 1; }
    Footer { background: #14171c; color: #7d8999; }
    #status { height: 2; padding: 0 2; color: #a9b5c4; background: #14171c; }
    #status.claude { border-left: solid #d78b65; }
    #status.codex { border-left: solid #35b9ad; }
    #history { height: 1fr; padding: 0 2; scrollbar-size: 1 1; }
    .user { margin: 1 0 0 0; padding: 1 1; background: #232831; color: #e9edf4; height: auto; }
    .agent { margin: 0 0 1 0; padding: 0 1 1 2; background: #14171c; height: auto; border-left: solid #35b9ad; }
    .agent.claude { border-left: solid #d78b65; }
    .agent.codex { border-left: solid #35b9ad; }
    .agent-label { height: auto; margin: 1 0 0 0; color: #8c9aa9; }
    .answer { height: auto; margin: 0; padding: 0; background: #14171c; }
    .steps { color: #8996a5; padding: 0 1 0 2; margin: 0; height: auto; }
    .hidden { display: none; }
    #composer-shell { height: 6; margin: 0 2; border: round #5a6574; background: #1e232b; }
    #composer-shell.claude { border: round #d78b65; }
    #composer-shell.codex { border: round #35b9ad; }
    #composer { height: 1fr; margin: 0 1; border: none; background: #1e232b; color: #f0f3f7; }
    #composer:focus { border: none; }
    #composer-info { height: 1; padding: 0 2; color: #8794a5; background: #1e232b; }
    #actions { height: 1; margin: 0 2; }
    #hint { width: 1fr; color: #7e8b9c; }
    #send { display: none; }
    """
    def __init__(self, root, tool, execute, command, model):
        super().__init__()
        self.root = Path(root)
        self.tool = tool
        self.execute_turn = execute
        self.execute_command = command
        self.model_name = model
        self.busy = False
        self.show_steps = False
        self.active_steps = None
        self.active_answer = None
        self.events = []
        self.current_tool = tool

    def compose(self) -> ComposeResult:
        yield Header()
        yield Static('', id='status')
        yield VerticalScroll(id='history')
        with Vertical(id='composer-shell'):
            yield PromptEditor(id='composer', language=None, soft_wrap=True)
            yield Static('Enter send · Ctrl+N new line', id='composer-info')
        with Horizontal(id='actions'):
            yield Static('Enter Send   Ctrl+N New line   Ctrl+T Agent   Ctrl+O Steps   Ctrl+D Diff', id='hint')
        yield Footer()

    def on_mount(self):
        self.refresh_status()
        self.query_one('#composer', TextArea).focus()

    def refresh_status(self, usage=None):
        color = COLORS[self.tool]
        for selector in ('#status', '#composer-shell'):
            widget = self.query_one(selector)
            widget.remove_class('claude', 'codex')
            widget.add_class(self.tool)
        self.query_one('#status', Static).update(
            f'[{color}][b]{self.tool.upper()}[/b][/]  ·  {self.model_name or "CLI default"}'
            f'   ·   {self.root.name}   ·   {token_summary(usage or {})}')
        self.query_one('#composer-info', Static).update(
            f'{self.tool.upper()} · {self.model_name or "CLI default"}  |  Enter send · Ctrl+N new line')

    def append(self, widget):
        history = self.query_one('#history', VerticalScroll)
        history.mount(widget)
        history.scroll_end(animate=False)

    def on_button_pressed(self, event: Button.Pressed):
        if event.button.id == 'send':
            self.action_send()

    def action_send(self):
        if self.busy:
            return
        editor = self.query_one('#composer', TextArea)
        prompt = editor.text.strip()
        if not prompt:
            return
        editor.load_text('')
        if prompt in ('/quit', '/exit'):
            self.exit()
            return
        if prompt.startswith('/'):
            self.execute_command(prompt)
            return
        self.append(Static('› ' + prompt, classes='user'))
        self.busy = True
        self.current_tool = self.tool
        self.events = []
        self.active_steps = Static('  Working · waiting for provider events…', classes='steps')
        self.append(self.active_steps)
        self.run_request(prompt)

    @work(thread=True, exclusive=True)
    def run_request(self, prompt):
        def event(obj):
            label = event_label(obj)
            if label:
                self.app.call_from_thread(self.record_step, label)
        try:
            result = self.execute_turn(prompt, event)
            self.app.call_from_thread(self.complete, result, None)
        except Exception as error:
            self.app.call_from_thread(self.complete, None, str(error))

    def record_step(self, label):
        self.events.append(label)
        if self.active_steps:
            self.active_steps.update('\n'.join('  · ' + step for step in self.events[-5:]))

    def complete(self, result, error):
        self.busy = False
        if self.active_steps:
            self.active_steps.set_class(not self.show_steps, 'hidden')
        if error:
            self.append(Static('Request failed: ' + error, classes='agent ' + self.current_tool))
            return
        if result is None:
            self.append(Static('Request failed. Check provider logs.', classes='agent ' + self.current_tool))
            return
        tool, model, usage, answer = result
        self.tool = tool
        self.model_name = model
        self.refresh_status(usage)
        color = COLORS[tool]
        panel = Vertical(classes='agent ' + tool)
        self.append(panel)
        panel.mount(Static(f'[{color}][b]{tool.upper()}[/b][/]  ·  {model}  ·  {token_summary(usage)}', classes='agent-label'))
        panel.mount(Markdown(answer or '(Empty response)', classes='answer'))
        self.query_one('#composer', TextArea).focus()

    def action_toggle_steps(self):
        self.show_steps = not self.show_steps
        for widget in self.query('.steps'):
            widget.set_class(not self.show_steps, 'hidden')

    def action_switch(self):
        if self.busy:
            return
        target = 'claude' if self.tool == 'codex' else 'codex'
        self.execute_command('/' + target)

    def action_diff(self):
        self.execute_command('/diff')
