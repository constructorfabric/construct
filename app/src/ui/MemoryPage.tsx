import {
  Alert,
  Button,
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
  Heading,
  Item,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemTitle,
  Spinner,
  Text,
} from '@gears-frontx/ui-kit';
import { format } from 'date-fns';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { useState } from 'react';
import type { ReactNode } from 'react';
import { reasonAgentNotRunning, reasonNotReady } from '../memory-protocol';
import { t } from '../translation';
import styles from './app.module.css';
import { buildTree, firstText } from './memory-tree';
import type { TreeFact, TreeField, TreeKind, TreeProperty, TreeValue } from './memory-tree';
import { useMemory } from './useMemory';

function storedText(storedAt: Date): string {
  return t('MemoryPage.Stored', { time: format(storedAt, 'd MMM yyyy, HH:mm') });
}

/** A part of the tree that opens and closes; it starts closed. */
function Branch({
  label,
  secondary,
  children,
}: {
  readonly label: string;
  readonly secondary?: string;
  readonly children: ReactNode;
}): ReactNode {
  const [open, setOpen] = useState(false);
  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <CollapsibleTrigger className={styles.branchTrigger}>
        <span className={styles.branchLabel}>
          {open ? <ChevronDown aria-hidden /> : <ChevronRight aria-hidden />}
          <span>{label}</span>
        </span>
        {secondary !== undefined && (
          <Text tone="muted" render={<span />}>
            {secondary}
          </Text>
        )}
      </CollapsibleTrigger>
      <CollapsibleContent className={styles.branchContent}>{children}</CollapsibleContent>
    </Collapsible>
  );
}

/** One value: a row for a text, a branch for a list or for an object with fields of its own. */
function ValueNode({
  label,
  value,
  stored,
}: {
  /** The name the value has; none for a value that stands in a list. */
  readonly label: string | null;
  readonly value: TreeValue;
  readonly stored?: Date;
}): ReactNode {
  const secondary = stored === undefined ? undefined : storedText(stored);
  if (value.kind === 'text') {
    return (
      <Item size="sm">
        <ItemContent>
          {label !== null && <ItemTitle>{label}</ItemTitle>}
          <ItemDescription lineClamp="none">{value.text}</ItemDescription>
          {secondary !== undefined && <ItemDescription>{secondary}</ItemDescription>}
        </ItemContent>
      </Item>
    );
  }
  if (value.kind === 'list') {
    return (
      <Branch label={t('MemoryPage.Count', { name: label ?? '', count: value.items.length })}>
        <ItemGroup>
          {value.items.map((item, index) => (
            <ValueNode key={index} label={null} value={item} />
          ))}
        </ItemGroup>
      </Branch>
    );
  }
  const summary = firstText(value);
  const named = label === null ? summary : summary === '' ? label : `${label}: ${summary}`;
  return (
    <Branch label={named === '' ? t('MemoryPage.Details') : named} secondary={secondary}>
      <Fields fields={value.fields} />
    </Branch>
  );
}

function Fields({ fields }: { readonly fields: readonly TreeField[] }): ReactNode {
  return (
    <ItemGroup>
      {fields.map((field, index) => (
        <ValueNode key={index} label={field.name} value={field.value} />
      ))}
    </ItemGroup>
  );
}

function FactRow({ name, fact }: { readonly name: string | null; readonly fact: TreeFact }) {
  return <ValueNode label={name} value={fact.value} stored={fact.storedAt} />;
}

/** A property: its one fact, or a list of its facts when there are several. */
function Property({ property }: { readonly property: TreeProperty }): ReactNode {
  const [only, ...rest] = property.facts;
  if (only !== undefined && rest.length === 0) {
    return <FactRow name={property.name} fact={only} />;
  }
  return (
    <Branch label={t('MemoryPage.Count', { name: property.name, count: property.facts.length })}>
      <ItemGroup>
        {property.facts.map((fact) => (
          <FactRow key={fact.id} name={null} fact={fact} />
        ))}
      </ItemGroup>
    </Branch>
  );
}

function Kind({ group }: { readonly group: TreeKind }): ReactNode {
  return (
    <section className={styles.memoryKind} aria-labelledby={`memory-kind-${group.kind}`}>
      <Heading id={`memory-kind-${group.kind}`} typography="heading-1" render={<h2 />}>
        {t(`MemoryPage.Kind.${group.kind}`)}
      </Heading>
      <ItemGroup>
        {group.properties.map((property) => (
          <Property key={property.key} property={property} />
        ))}
      </ItemGroup>
    </section>
  );
}

function CollectionLine({
  collecting,
  onOpenSettings,
}: {
  readonly collecting: boolean | null;
  readonly onOpenSettings: () => void;
}): ReactNode {
  return (
    <Alert id="memory-collection" data-collecting={String(collecting)}>
      <span className={styles.alertContent}>
        {collecting === null
          ? t('MemoryPage.Collection.Unknown')
          : collecting
            ? t('MemoryPage.Collection.On')
            : t('MemoryPage.Collection.Off')}
        <Button id="memory-open-settings" variant="outline" size="sm" onClick={onOpenSettings}>
          {t('MemoryPage.OpenSettings')}
        </Button>
      </span>
    </Alert>
  );
}

function unavailableText(reason: string): string {
  if (reason === reasonAgentNotRunning) {
    return t('MemoryPage.Unavailable.Agent');
  }
  if (reason === reasonNotReady) {
    return t('MemoryPage.Unavailable.NotReady');
  }
  return t('MemoryPage.Unavailable.Other', { reason });
}

/** What the person's profile holds, as a tree the person can open. */
export function MemoryPage({ onOpenSettings }: { readonly onOpenSettings: () => void }): ReactNode {
  const state = useMemory();
  return (
    <>
      <Heading typography="display" render={<h1 />}>
        {t('MemoryPage.Title')}
      </Heading>
      {state.kind === 'reading' ? (
        <Spinner id="memory-reading" label={t('MemoryPage.Reading')} />
      ) : state.kind === 'failed' ? (
        <Alert id="memory-failed" variant="destructive">
          {unavailableText(state.reason)}
        </Alert>
      ) : (
        <>
          <CollectionLine collecting={state.collecting} onOpenSettings={onOpenSettings} />
          {state.facts.length === 0 ? (
            <Empty id="memory-empty">
              <EmptyHeader>
                <EmptyTitle render={<h2 />}>{t('MemoryPage.Empty.Title')}</EmptyTitle>
                <EmptyDescription>
                  {state.collecting === false
                    ? t('MemoryPage.Empty.Off')
                    : state.collecting === true
                      ? t('MemoryPage.Empty.On')
                      : t('MemoryPage.Empty.Unknown')}
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            <div id="memory-tree">
              {buildTree(state.facts).map((group) => (
                <Kind key={group.kind} group={group} />
              ))}
            </div>
          )}
        </>
      )}
    </>
  );
}
