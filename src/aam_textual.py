"""Full-screen Textual UI. Optional dependency: pip install textual."""
import json
import shutil
import threading
from pathlib import Path
from textual.app import App, ComposeResult
from textual.containers import VerticalScroll, Horizontal, Vertical
from textual.widgets import Header, Footer, Static, TextArea, Markdown, Button
from textual.binding import Binding
from textual import work
from aam_tui import event_label, token_summary, provider_model, provider_usage, step_detail, streamed_answer

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
        Binding('ctrl+h', 'history', 'History', show=False),
        Binding('escape', 'cancel', 'Cancel', show=True),
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
    .user.claude { background: #30231f; border-left: solid #d78b65; }
    .user.codex { background: #172d2c; border-left: solid #35b9ad; }
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
        self.active_prompt = None
        self.cancel_event = None
        self.session_usage = {'input_tokens': 0, 'cached_input_tokens': 0, 'output_tokens': 0}
        self.live_usage = {}
        self.last_usage = {}
        self.last_answer = None
        self.last_model = None
        self.preview = None
        self.preview_text = ''
        self.architect_preview = None
        self.command_candidates = ('/claude', '/codex', '/new', '/status', '/context', '/analytics', '/checkpoint', '/recover', '/history', '/tokens', '/diff', '/updates', '/architect', '/architect-apply', '/help', '/quit')

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
            if prompt == '/help':
                self.append(Static('Commands: ' + '  '.join(self.command_candidates)))
            else:
                self.execute_command(prompt)
            return
        self.active_prompt = Static('› ' + prompt, classes='user ' + self.tool)
        self.append(self.active_prompt)
        self.busy = True
        self.cancel_event = threading.Event()
        self.current_tool = self.tool
        self.events = []
        self.live_usage = {}
        self.preview = None
        self.preview_text = ''
        self.active_steps = Static('  Working · waiting for provider events…', classes='steps')
        self.append(self.active_steps)
        self.run_request(prompt)

    @work(thread=True, exclusive=True)
    def run_request(self, prompt):
        def event(obj):
            self.app.call_from_thread(self.record_event, obj)
        try:
            result = self.execute_turn(prompt, event, self.cancel_event)
            self.app.call_from_thread(self.complete, result, None)
        except Exception as error:
            self.app.call_from_thread(self.complete, None, str(error))

    def record_event(self, obj):
        label = step_detail(obj)
        if label:
            self.events.append(label)
        model = provider_model(obj)
        if model:
            self.model_name = model
            self.last_model = model
        usage = provider_usage(obj)
        if usage:
            self.live_usage.update(usage)
        snapshot = streamed_answer(obj)
        if snapshot is not None and snapshot != self.preview_text:
            self.preview_text = snapshot
            if self.preview is None:
                self.preview = Static('', classes='steps')
                self.append(self.preview)
            self.preview.update(snapshot[-6000:])
        if label or model or usage:
            self.refresh_status(self.live_usage)
        if self.active_steps:
            self.active_steps.update('\n'.join('  · ' + step for step in self.events[-5:]))

    def complete(self, result, error):
        self.busy = False
        self.cancel_event = None
        if self.active_steps:
            self.active_steps.set_class(not self.show_steps, 'hidden')
        if self.preview is not None:
            self.preview.remove()
            self.preview = None
        if error:
            self.append(Static('Request failed: ' + error, classes='agent ' + self.current_tool))
            return
        if result is None:
            self.append(Static('Request failed. Check provider logs.', classes='agent ' + self.current_tool))
            return
        tool, model, usage, answer = result
        if self.active_prompt is not None:
            self.active_prompt.remove_class('claude', 'codex')
            self.active_prompt.add_class(tool)
            self.active_prompt = None
        self.tool = tool
        self.model_name = model
        self.last_usage = usage
        for key in self.session_usage:
            self.session_usage[key] += usage.get(key, 0) if isinstance(usage.get(key), int) else 0
        self.last_answer = answer
        self.refresh_status(usage)
        color = COLORS[tool]
        panel = Vertical(classes='agent ' + tool)
        self.append(panel)
        panel.mount(Static(f'[{color}][b]{tool.upper()}[/b][/]  ·  {model}  ·  {token_summary(usage)}', classes='agent-label'))
        panel.mount(Markdown(answer or '(Empty response)', classes='answer'))
        self.query_one('#composer', TextArea).focus()

    def on_text_area_changed(self, event: TextArea.Changed):
        text = event.text_area.text
        if text.startswith('/') and '\n' not in text:
            options = [cmd for cmd in self.command_candidates if cmd.startswith(text)]
            self.query_one('#composer-info', Static).update('  '.join(options[:5]) or 'Unknown command')
        else:
            self.query_one('#composer-info', Static).update(
                f'{self.tool.upper()} · Enter send · Ctrl+N new line · /help commands')

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

    def action_history(self):
        self.execute_command('/history')

    def action_cancel(self):
        if self.busy and self.cancel_event is not None:
            self.cancel_event.set()
            if self.active_steps:
                self.active_steps.update('  Cancelling… inspect changes before retrying')
