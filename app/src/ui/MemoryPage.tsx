import {
  Alert,
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
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
import { ChevronDown, ChevronRight, Trash2 } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { isDeletableFactId, reasonAgentNotRunning, reasonNotReady } from '../memory-protocol';
import { t } from '../translation';
import styles from './app.module.css';
import { buildTree, firstText } from './memory-tree';
import type { TreeFact, TreeField, TreeKind, TreeProperty, TreeValue } from './memory-tree';
import { useMemory } from './useMemory';
import type { DeleteNotice } from './useMemory';

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

/** The longest text of a fact that a question or a button name quotes. */
const maxQuoted = 80;

/** What names a fact to the person: its property and its first text, cut when long. */
function factName(property: string, fact: TreeFact): string {
  const text = firstText(fact.value);
  const quoted = text.length > maxQuoted ? `${text.slice(0, maxQuoted)}…` : text;
  return quoted === '' ? property : `${property}: ${quoted}`;
}

/** Delete with a question first; the fact stays until the person says yes. */
function DeleteFact({
  name,
  disabled,
  onDelete,
}: {
  readonly name: string;
  /** A delete is waiting for the gear. */
  readonly disabled: boolean;
  readonly onDelete: () => void;
}): ReactNode {
  return (
    <AlertDialog>
      <AlertDialogTrigger
        render={
          <Button
            variant="ghost"
            size="sm"
            icon={<Trash2 />}
            disabled={disabled}
            aria-label={t('MemoryPage.Delete', { fact: name })}
          />
        }
      />
      <AlertDialogContent size="sm">
        <AlertDialogHeader>
          <AlertDialogTitle>{t('MemoryPage.Delete.ConfirmTitle', { fact: name })}</AlertDialogTitle>
          <AlertDialogDescription>{t('MemoryPage.Delete.ConfirmText')}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{t('MemoryPage.Delete.No')}</AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={onDelete}>
            {t('MemoryPage.Delete.Yes')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

function FactRow({
  name,
  property,
  fact,
  deleting,
  onDelete,
}: {
  readonly name: string | null;
  /** The property's plain name, which names the fact when its row has no label. */
  readonly property: string;
  readonly fact: TreeFact;
  readonly deleting: boolean;
  readonly onDelete: (factId: string, name: string) => void;
}) {
  const factLabel = factName(property, fact);
  return (
    <div className={styles.factRow}>
      <div className={styles.factBody}>
        <ValueNode label={name} value={fact.value} stored={fact.storedAt} />
      </div>
      {/* An id the app cannot send to the gear has no button: it could never succeed. */}
      {isDeletableFactId(fact.id) && (
        <DeleteFact
          name={factLabel}
          disabled={deleting}
          onDelete={() => {
            onDelete(fact.id, factLabel);
          }}
        />
      )}
    </div>
  );
}

/** A property: its one fact, or a list of its facts when there are several. */
function Property({
  property,
  deleting,
  onDelete,
}: {
  readonly property: TreeProperty;
  readonly deleting: boolean;
  readonly onDelete: (factId: string, name: string) => void;
}): ReactNode {
  const [only, ...rest] = property.facts;
  if (only !== undefined && rest.length === 0) {
    return (
      <FactRow
        name={property.name}
        property={property.name}
        fact={only}
        deleting={deleting}
        onDelete={onDelete}
      />
    );
  }
  return (
    <Branch label={t('MemoryPage.Count', { name: property.name, count: property.facts.length })}>
      <ItemGroup>
        {property.facts.map((fact) => (
          <FactRow
            key={fact.id}
            name={null}
            property={property.name}
            fact={fact}
            deleting={deleting}
            onDelete={onDelete}
          />
        ))}
      </ItemGroup>
    </Branch>
  );
}

function Kind({
  group,
  deleting,
  onDelete,
}: {
  readonly group: TreeKind;
  readonly deleting: boolean;
  readonly onDelete: (factId: string, name: string) => void;
}): ReactNode {
  return (
    <section className={styles.memoryKind} aria-labelledby={`memory-kind-${group.kind}`}>
      <Heading id={`memory-kind-${group.kind}`} typography="heading-1" render={<h2 />}>
        {t(`MemoryPage.Kind.${group.kind}`)}
      </Heading>
      <ItemGroup>
        {group.properties.map((property) => (
          <Property
            key={property.key}
            property={property}
            deleting={deleting}
            onDelete={onDelete}
          />
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

function DeleteLine({ notice }: { readonly notice: DeleteNotice }): ReactNode {
  const done = useRef<HTMLDivElement>(null);
  // The deleted fact's button is gone, so the focus goes to what was done instead of the page's top.
  useEffect(() => {
    if (notice.kind === 'deleted') {
      done.current?.focus();
    }
  }, [notice]);
  return notice.kind === 'deleted' ? (
    <div id="memory-delete-done" ref={done} tabIndex={-1} className={styles.deleteDone}>
      <Alert role="status">{t('MemoryPage.Delete.Done')}</Alert>
    </div>
  ) : (
    <Alert id="memory-delete-failed" variant="destructive" role="alert">
      {t('MemoryPage.Delete.Failed', { fact: notice.fact, reason: notice.reason })}
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
  const { state, notice, deleting, remove } = useMemory();
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
          {notice !== null && <DeleteLine notice={notice} />}
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
                <Kind
                  key={group.kind}
                  group={group}
                  deleting={deleting}
                  onDelete={(factId, name) => {
                    void remove(factId, name);
                  }}
                />
              ))}
            </div>
          )}
        </>
      )}
    </>
  );
}
