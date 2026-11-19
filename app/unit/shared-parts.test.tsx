import { SidebarProvider } from '@gears-frontx/ui-kit';
import { renderToStaticMarkup } from 'react-dom/server';
import type { ReactNode } from 'react';
import { describe, expect, it } from 'vitest';
import { ButtonRow } from '../src/ui/components/ButtonRow';
import { ControlRow } from '../src/ui/components/ControlRow';
import { PageFrame } from '../src/ui/components/PageFrame';
import { Section } from '../src/ui/components/Section';
import { SwitchRow } from '../src/ui/components/SwitchRow';
import { TextFieldRow } from '../src/ui/components/TextFieldRow';

const html = (node: ReactNode): string => renderToStaticMarkup(node);
const noop = (): void => undefined;

describe('PageFrame', () => {
  it('puts a page in one column inside a main element', () => {
    const out = html(
      <SidebarProvider>
        <PageFrame>
          <p>Body</p>
        </PageFrame>
      </SidebarProvider>,
    );
    expect(out).toMatch(/<main[^>]*class="[^"]*frame/);
    expect(out).toContain('class="column"');
    expect(out).toContain('<p>Body</p>');
  });

  it('gives a chat the whole window and no column', () => {
    const out = html(
      <SidebarProvider>
        <PageFrame layout="chat">
          <p>Body</p>
        </PageFrame>
      </SidebarProvider>,
    );
    expect(out).toContain('frameChat');
    expect(out).not.toContain('class="column"');
  });

  it('shows the banner above the page, and nothing when there is none', () => {
    const frame = (banner: ReactNode): string =>
      html(
        <SidebarProvider>
          <PageFrame banner={banner}>
            <p>Body</p>
          </PageFrame>
        </SidebarProvider>,
      );
    expect(frame(<b>Warning</b>)).toContain('banner');
    expect(frame(null)).not.toContain('banner');
  });
});

describe('Section', () => {
  it('is named by its heading and shows the notice, the fields and the buttons in order', () => {
    const out = html(
      <Section
        titleId="s-title"
        title="Model"
        description="Under the title"
        notice="A notice"
        noticeId="s-notice"
        actions={<button>Save</button>}
        after={<i>Done</i>}
      >
        <span>Field</span>
      </Section>,
    );
    expect(out).toContain('aria-labelledby="s-title"');
    expect(out).toContain('id="s-title"');
    const order = ['Model', 'Under the title', 'A notice', 'Field', 'Save', 'Done'].map((text) =>
      out.indexOf(text),
    );
    expect(order.every((at) => at >= 0)).toBe(true);
    expect(order).toEqual([...order].sort((a, b) => a - b));
    expect(out).not.toContain('<form');
  });

  it('shows no notice and no buttons that it was not given', () => {
    const out = html(
      <Section titleId="s-title" title="Model" notice={null}>
        <span>Field</span>
      </Section>,
    );
    expect(out).not.toContain('role="status"');
    expect(out).not.toContain('buttonRow');
  });

  it('makes one form of the fields and the buttons when it can be submitted', () => {
    const out = html(
      <Section titleId="s-title" title="Model" onSubmit={noop} actions={<button>Save</button>}>
        <span>Field</span>
      </Section>,
    );
    expect(out).toMatch(/<form[^>]*>.*Field.*Save.*<\/form>/s);
  });
});

describe('TextFieldRow', () => {
  const row = (props: Partial<Parameters<typeof TextFieldRow>[0]> = {}): string =>
    html(<TextFieldRow id="f" label="Address" value="abc" onChange={noop} {...props} />);

  it('joins the label to the input and shows the value', () => {
    const out = row();
    expect(out).toMatch(/<label[^>]*for="f"[^>]*>Address<\/label>/);
    expect(out).toMatch(/<input[^>]*id="f"[^>]*value="abc"/);
  });

  it('shows the hint when it has one', () => {
    expect(row({ hint: 'A hint' })).toContain('A hint');
    expect(row()).not.toContain('A hint');
  });

  it('marks the field invalid and shows the message only with an error', () => {
    const bad = row({ error: 'Required' });
    expect(bad).toContain('aria-invalid="true"');
    expect(bad).toContain('Required');
    expect(row()).not.toContain('aria-invalid');
  });

  it('can be a password field and can be disabled', () => {
    const out = row({ type: 'password', disabled: true });
    expect(out).toContain('type="password"');
    expect(out).toMatch(/<input[^>]*disabled/);
    expect(row()).not.toMatch(/<input[^>]*disabled/);
  });
});

describe('SwitchRow', () => {
  const row = (props: Partial<Parameters<typeof SwitchRow>[0]> = {}): string =>
    html(<SwitchRow id="sw" label="Collect" checked={false} onChange={noop} {...props} />);

  it('names the switch by its label', () => {
    const out = row();
    expect(out).toContain('id="sw-label"');
    expect(out).toContain('aria-labelledby="sw-label"');
  });

  it('describes the switch by its description, when it has one', () => {
    const out = row({ description: 'What it does' });
    expect(out).toContain('id="sw-hint"');
    expect(out).toContain('aria-describedby="sw-hint"');
    expect(row()).not.toContain('sw-hint');
  });

  it('shows the state it is given and can be disabled', () => {
    expect(row({ checked: true })).toContain('aria-checked="true"');
    expect(row({ disabled: true })).toMatch(/role="switch"[^>]*aria-disabled="true"/);
    expect(row()).not.toContain('aria-disabled');
  });
});

describe('ControlRow', () => {
  it('names the label and the description by the row id and puts the control after them', () => {
    const out = html(
      <ControlRow id="r" label="Chats" description="What it does">
        <button>Go</button>
      </ControlRow>,
    );
    expect(out).toContain('id="r-label"');
    expect(out).toContain('id="r-hint"');
    expect(out.indexOf('Chats')).toBeLessThan(out.indexOf('What it does'));
    expect(out.indexOf('What it does')).toBeLessThan(out.indexOf('<button>Go'));
  });

  it('joins the label to a form control only when it is given one', () => {
    const joined = html(
      <ControlRow id="r" label="Chats" labelFor="c">
        <input id="c" />
      </ControlRow>,
    );
    expect(joined).toMatch(/<label[^>]*for="c"/);
    expect(
      html(
        <ControlRow id="r" label="Chats">
          x
        </ControlRow>,
      ),
    ).not.toContain('for=');
  });
});

describe('ButtonRow', () => {
  it('holds its buttons in one row', () => {
    const out = html(
      <ButtonRow>
        <button>One</button>
        <button>Two</button>
      </ButtonRow>,
    );
    expect(out).toMatch(/class="buttonRow"><button>One<\/button><button>Two<\/button>/);
  });
});
