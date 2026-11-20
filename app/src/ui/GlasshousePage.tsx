import { Button, Input } from '@gears-frontx/ui-kit';
import { ArrowLeft, ArrowRight, RotateCw } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import type { FormEvent, ReactNode } from 'react';
import type { GlasshouseProblem, GlasshouseState } from '../glasshouse';
import { t } from '../translation';
import styles from './glasshouse.module.css';

const problemTexts = {
  'not-found': 'Glasshouse.Problem.NotFound',
  refused: 'Glasshouse.Problem.Refused',
  offline: 'Glasshouse.Problem.Offline',
  certificate: 'Glasshouse.Problem.Certificate',
  crashed: 'Glasshouse.Problem.Crashed',
  other: 'Glasshouse.Problem.Other',
} as const satisfies Record<GlasshouseProblem['kind'], string>;

/**
 * A browser: an address bar with back, forward and reload, and the web page below it. The page is
 * a view of the main process that this component only gives a box to, so it keeps running while
 * another page of the app is open.
 */
export function GlasshousePage(): ReactNode {
  const [state, setState] = useState<GlasshouseState | null>(null);
  const [draft, setDraft] = useState('');
  const [refused, setRefused] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const address = state?.address ?? '';

  // The address bar follows the page, redirects included.
  useEffect(() => {
    setDraft(address);
  }, [address]);

  useEffect(() => {
    const api = window.constructAgent;
    const element = box.current;
    if (element === null) {
      return undefined;
    }
    const place = (): void => {
      const rect = element.getBoundingClientRect();
      void api
        .showGlasshouse({ x: rect.x, y: rect.y, width: rect.width, height: rect.height })
        .then(setState);
    };
    place();
    const observer = new ResizeObserver(place);
    observer.observe(element);
    window.addEventListener('resize', place);
    const stop = api.onGlasshouseState(setState);
    return () => {
      observer.disconnect();
      window.removeEventListener('resize', place);
      stop();
      void api.hideGlasshouse();
    };
  }, []);

  const submit = (event: FormEvent): void => {
    event.preventDefault();
    void window.constructAgent.goGlasshouse(draft).then((result) => {
      setRefused(result === 'refused');
    });
  };
  const step = (which: 'back' | 'forward' | 'reload') => () => {
    void window.constructAgent.stepGlasshouse(which);
  };

  const problem = state?.problem ?? null;
  const empty = state !== null && state.noStartAddress && address === '' && problem === null;
  return (
    <div className={styles.page}>
      <form className={styles.bar} onSubmit={submit}>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          aria-label={t('Glasshouse.Back')}
          disabled={!state?.canGoBack}
          onClick={step('back')}
        >
          <ArrowLeft aria-hidden />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          aria-label={t('Glasshouse.Forward')}
          disabled={!state?.canGoForward}
          onClick={step('forward')}
        >
          <ArrowRight aria-hidden />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          aria-label={t('Glasshouse.Reload')}
          onClick={step('reload')}
        >
          <RotateCw aria-hidden />
        </Button>
        <Input
          id="glasshouse-address"
          aria-label={t('Glasshouse.Address')}
          aria-invalid={refused || undefined}
          spellCheck={false}
          autoCapitalize="off"
          value={draft}
          onValueChange={(value) => {
            setDraft(value);
            setRefused(false);
          }}
        />
      </form>
      {refused ? (
        <p id="glasshouse-refused" role="alert" className={styles.note}>
          {t('Glasshouse.Refused')}
        </p>
      ) : null}
      <div ref={box} className={styles.view}>
        {problem === null ? null : (
          <div id="glasshouse-problem" role="alert" className={styles.message}>
            <p>{t(problemTexts[problem.kind])}</p>
            <Button type="button" variant="outline" size="sm" onClick={step('reload')}>
              {t('Glasshouse.Reload')}
            </Button>
          </div>
        )}
        {empty ? (
          <p id="glasshouse-empty" className={styles.message}>
            {t('Glasshouse.NoStartAddress')}
          </p>
        ) : null}
      </div>
    </div>
  );
}
