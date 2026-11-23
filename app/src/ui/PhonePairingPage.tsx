import { Alert, Button, Heading, Spinner, Text } from '@gears-frontx/ui-kit';
import { QRCodeSVG } from 'qrcode.react';
import { useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import type { PairingOffer, PhonePage } from '../phone-protocol';
import { startPoller } from '../status-poller';
import { t } from '../translation';
import styles from './app.module.css';
import { ButtonRow } from './components/ButtonRow';

type OfferState =
  | { readonly kind: 'opening' | 'ended' }
  | { readonly kind: 'available'; readonly offer: PairingOffer }
  | { readonly kind: 'failed'; readonly error: string };

export function PhonePairingPage({
  onBack,
  onClosed,
}: {
  readonly onBack: () => void;
  readonly onClosed: (failed: boolean) => void;
}): ReactNode {
  const [pairing, setPairing] = useState<OfferState>({ kind: 'opening' });
  const [attempt, setAttempt] = useState(0);
  const [rosterPage, setRosterPage] = useState<PhonePage | null>(null);
  const [cursors, setCursors] = useState<readonly (string | null)[]>([null]);
  const [error, setError] = useState<string | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const listVersion = useRef(0);
  const after = cursors.at(-1) ?? null;
  const next = rosterPage?.next ?? null;

  useEffect(() => {
    let active = true;
    void window.constructAgent.openPhonePairing().then((result) => {
      if (!active) return;
      setPairing(
        result.ok ? { kind: 'available', offer: result } : { kind: 'failed', error: result.error },
      );
    });
    return () => {
      active = false;
    };
  }, [attempt]);

  useEffect(
    () => () => {
      // The main process orders this after any open already sent, independently of the roster.
      void window.constructAgent.closePhonePairing().then(
        (result) => onClosed(!result.ok),
        () => onClosed(true),
      );
    },
    [onClosed],
  );

  useEffect(() => {
    if (pairing.kind !== 'available') return;
    const { offer } = pairing;
    const endOffer = () => {
      setPairing((current) =>
        current.kind === 'available' && current.offer === offer ? { kind: 'ended' } : current,
      );
    };
    const expiry = window.setTimeout(endOffer, Math.min(offer.expiresIn * 1000, 2_147_483_647));
    const status = startPoller(
      () => window.constructAgent.getPhonePairingStatus(),
      (result) => {
        if (result.ok && !result.active) endOffer();
      },
      1000,
    );
    return () => {
      window.clearTimeout(expiry);
      status.stop();
    };
  }, [pairing]);

  useEffect(() => {
    let version = 0;
    const roster = startPoller(
      () => {
        version = ++listVersion.current;
        return window.constructAgent.listPhones(after);
      },
      (result) => {
        if (version !== listVersion.current) return;
        if (result.ok) {
          setRosterPage(result);
          setListError(null);
        } else {
          setListError(result.error);
        }
      },
      5000,
      () => {
        if (version === listVersion.current) setListError(t('PhonePairing.ListFailed'));
      },
    );
    return roster.stop;
  }, [after]);

  const showPage = (pageCursors: readonly (string | null)[]): void => {
    ++listVersion.current;
    setRosterPage(null);
    setListError(null);
    setCursors(pageCursors);
  };

  const unpair = async (id: string): Promise<void> => {
    setBusy(id);
    setError(null);
    ++listVersion.current;
    const result = await window.constructAgent.unpairPhone(id);
    ++listVersion.current;
    if (result.ok) {
      setRosterPage((current) =>
        current === null
          ? null
          : { ...current, phones: current.phones.filter((phone) => phone.id !== id) },
      );
    } else setError(result.error);
    setBusy(null);
  };

  return (
    <div className={styles.phonePairing}>
      <Button id="phone-pairing-back" variant="secondary" onClick={onBack}>
        {t('PhonePairing.Back')}
      </Button>
      <Heading typography="display" render={<h1 />}>
        {t('PhonePairing.Title')}
      </Heading>
      <Text tone="muted">{t('PhonePairing.Instructions')}</Text>
      {error !== null && (
        <Alert variant="destructive" role="alert">
          {error}
        </Alert>
      )}
      {pairing.kind === 'opening' && <Spinner label={t('PhonePairing.Loading')} />}
      {pairing.kind === 'available' && (
        <div className={styles.phonePairingDetails}>
          <QRCodeSVG
            id="phone-pairing-qr"
            value={pairing.offer.payload}
            size={200}
            marginSize={4}
            title={t('PhonePairing.QrLabel')}
          />
          <div>
            <p>
              {t('PhonePairing.Code')}:{' '}
              <strong id="phone-pairing-code">{pairing.offer.code}</strong>
            </p>
            <p>
              {t('PhonePairing.Address')}:{' '}
              <span id="phone-pairing-address">{pairing.offer.address}</span>
            </p>
          </div>
        </div>
      )}
      {pairing.kind === 'failed' && (
        <Alert variant="destructive" role="alert">
          {pairing.error}
        </Alert>
      )}
      {pairing.kind === 'ended' && <Text role="status">{t('PhonePairing.OfferEnded')}</Text>}
      {(pairing.kind === 'failed' || pairing.kind === 'ended') && (
        <Button
          onClick={() => {
            setPairing({ kind: 'opening' });
            setAttempt((current) => current + 1);
          }}
        >
          {t('PhonePairing.NewCode')}
        </Button>
      )}
      <section aria-labelledby="paired-phones-title">
        <Heading id="paired-phones-title" typography="heading-1" render={<h2 />}>
          {t('PhonePairing.PairedPhones')}
        </Heading>
        {listError !== null && (
          <Alert variant="destructive" role="alert">
            {listError}
          </Alert>
        )}
        {(cursors.length > 1 || next !== null) && (
          <ButtonRow>
            <Button
              variant="outline"
              disabled={cursors.length === 1 || busy !== null}
              onClick={() => showPage(cursors.slice(0, -1))}
            >
              {t('PhonePairing.Previous')}
            </Button>
            <Button
              variant="outline"
              disabled={next === null || busy !== null}
              onClick={() => {
                if (next !== null) showPage([...cursors, next]);
              }}
            >
              {t('PhonePairing.Next')}
            </Button>
          </ButtonRow>
        )}
        {rosterPage === null && listError === null && (
          <Spinner label={t('PhonePairing.ListLoading')} />
        )}
        {rosterPage?.phones.length === 0 && (
          <Text tone="muted">
            {t(after === null ? 'PhonePairing.Empty' : 'PhonePairing.EmptyPage')}
          </Text>
        )}
        <ul id="phone-pairing-list" className={styles.phoneList}>
          {rosterPage?.phones.map((phone) => {
            const pairedAt = new Date(phone.pairedAt * 1000);
            return (
              <li key={phone.id} className={styles.phoneRow}>
                <div>
                  <strong>{phone.name}</strong>
                  <p>
                    <time dateTime={pairedAt.toISOString()}>
                      {t('PhonePairing.PairedAt', {
                        date: pairedAt.toLocaleString(),
                      })}
                    </time>
                  </p>
                </div>
                <Button
                  variant="outline"
                  aria-label={t('PhonePairing.UnpairName', { name: phone.name })}
                  disabled={busy !== null}
                  loading={busy === phone.id}
                  onClick={() => void unpair(phone.id)}
                >
                  {t('PhonePairing.Unpair')}
                </Button>
              </li>
            );
          })}
        </ul>
      </section>
    </div>
  );
}
