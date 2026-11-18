"""Optional Rich terminal presentation for AI-Agent-Manager.

A live, transient panel shows provider events. The final answer remains in scrollback.
No model reasoning, command arguments, or secrets are exposed by this UI.
"""
from contextlib import nullcontext
import sys
import time


def event_label(event):
    kind = event.get('type', '')
    item = event.get('item') or {}
    category = item.get('type', '')
    names = {'command_execution': 'Command', 'file_change': 'File changes',
             'mcp_tool_call': 'Tool call', 'web_search': 'Web search',
             'agent_message': 'Response'}
    if kind == 'turn.started':
        return 'Turn started'
    if kind == 'turn.failed':
        return 'Turn failed'
    if kind == 'turn.completed':
        return 'Turn completed'
    if kind in ('item.started', 'item.completed', 'item.updated'):
        action = kind.split('.')[1]
        return f'{names.get(category, "Tool")} · {action}'
    return None


def token_summary(usage):
    if not usage:
        return 'Tokens: not reported'
    keys = (('input_tokens', 'input'), ('cached_input_tokens', 'cached'),
            ('cache_read_input_tokens', 'cache read'),
            ('cache_creation_input_tokens', 'cache write'),
            ('output_tokens', 'output'))
    return 'Tokens: ' + ' · '.join(f'{label} {usage[key]:,}' for key, label in keys
                                   if isinstance(usage.get(key), int))


class RichTurn:
    def __init__(self, tool, model):
        from rich.console import Console
        from rich.live import Live
        self.console = Console()
        self.live_type = Live
        self.tool = tool
        self.model = model or 'CLI default'
        self.started = time.monotonic()
        self.steps = []
        self.usage = {}

    def render(self):
        from rich.panel import Panel
        from rich.text import Text
        body = Text()
        for step in self.steps[-7:]:
            body.append('  • ' + step + '\\n', style='dim')
        if not self.steps:
            body.append('Waiting for provider events…\\n', style='dim')
        body.append(f'\\nElapsed: {time.monotonic()-self.started:.0f}s', style='cyan')
        body.append('\\n' + token_summary(self.usage), style='bright_cyan')
        return Panel(body, title=f'[bold cyan]{self.tool.upper()}[/]  [bold]{self.model}[/]',
                     subtitle='Live activity · hidden after completion', border_style='cyan')

    def __enter__(self):
        self.live = self.live_type(self.render(), console=self.console, transient=True,
                                   refresh_per_second=4)
        self.live.__enter__()
        return self

    def __exit__(self, *args):
        return self.live.__exit__(*args)

    def event(self, obj):
        label = event_label(obj)
        if label:
            self.steps.append(label)
        usage = obj.get('usage')
        if isinstance(usage, dict):
            self.usage = usage
        self.live.update(self.render())

    def finish(self, tool, model, usage, answer):
        from rich.panel import Panel
        from rich.markdown import Markdown
        self.console.print(f'[bold green]✓ {tool.upper()}[/] [bold]{model}[/]  ·  {token_summary(usage)}')
        self.console.print(Panel(Markdown(answer or '(Empty response)'), border_style='green',
                                 title='Answer'))


def rich_available():
    try:
        import rich
        return True
    except ImportError:
        return False
