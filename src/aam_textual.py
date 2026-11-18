"""Full-screen Textual UI. Optional dependency: pip install textual."""
import json
import shutil
from pathlib import Path
from textual.app import App, ComposeResult
from textual.containers import VerticalScroll, Horizontal
from textual.widgets import Header, Footer, Static, TextArea, Markdown, Button
from textual.binding import Binding
from textual import work
from aam_tui import event_label, token_summary

COLORS = {'claude': '#D78B65', 'codex': '#35B9AD'}

class AgentApp(App):
    TITLE = 'AI Agent Manager'
    BINDINGS = [
        Binding('ctrl+enter', 'send', 'Send', show=True),
        Binding('ctrl+o', 'toggle_steps', 'Steps', show=True),
        Binding('ctrl+d', 'diff', 'Diff', show=True),
        Binding('ctrl+t', 'switch', 'Agent', show=True),
        Binding('ctrl+q', 'quit', 'Quit', show=True),
    ]
    CSS = """
    Screen { background: #11151b; color: #e7edf5; }
    #status { height: 3; padding: 1 2; background: #202833; }
    #history { height: 1fr; padding: 1 2; }
    .user { margin: 1 0; padding: 1 2; background: #242d39; }
    .agent { margin: 1 0; padding: 1 2; background: #1b222b; height: auto; }
    .agent.claude { border: round #d78b65; }
    .agent.codex { border: round #35b9ad; }
    #status.claude { border-left: thick #d78b65; background: #30231f; }
    #status.codex { border-left: thick #35b9ad; background: #172d2c; }
    #composer.claude { border: round #d78b65; }
    #composer.codex { border: round #35b9ad; }
    #send.claude { background: #a95d3d; }
    #send.codex { background: #168d83; }
    .steps { color: #8e9aab; padding: 0 1; }
    .hidden { display: none; }
    #composer { height: 6; margin: 0 2; border: round #65738a; }
    #actions { height: 3; padding: 0 2; }
    #hint { width: 1fr; color: #93a2b5; padding: 1 0; }
    Button { min-width: 10; }
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
        yield TextArea(id='composer', language=None, soft_wrap=True)
        with Horizontal(id='actions'):
            yield Static('Ctrl+Enter Send  ·  Ctrl+T Agent  ·  Ctrl+O Steps  ·  Ctrl+D Diff', id='hint')
            yield Button('Send', id='send', variant='primary')
        yield Footer()

    def on_mount(self):
        self.refresh_status()
        self.query_one('#composer', TextArea).focus()

    def refresh_status(self, usage=None):
        color = COLORS[self.tool]
        for selector in ('#status', '#composer', '#send'):
            widget = self.query_one(selector)
            widget.remove_class('claude', 'codex')
            widget.add_class(self.tool)
        self.query_one('#status', Static).update(
            f'[{color}][b]{self.tool.upper()}[/b][/]  ·  {self.model_name or "CLI default"}'
            f'   |   {self.root.name}   |   {token_summary(usage or {})}')

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
        self.append(Static(prompt, classes='user'))
        self.busy = True
        self.current_tool = self.tool
        self.events = []
        self.active_steps = Static('Waiting for provider events…', classes='steps')
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
            self.active_steps.update('\n'.join('• ' + s for s in self.events[-12:]))

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
        panel = VerticalScroll(classes='agent ' + tool)
        self.append(panel)
        panel.mount(Static(f'[{color}][b]{tool.upper()}[/b][/]  ·  {model}  ·  {token_summary(usage)}'))
        panel.mount(Markdown(answer or '(Empty response)'))
        panel.styles.height = 'auto'
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
