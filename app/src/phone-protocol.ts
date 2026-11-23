import { isCount, isRecord, parseLine, replyError } from './agent-json';

export interface Phone {
  readonly id: string;
  readonly name: string;
  readonly pairedAt: number;
}
type Failure = { readonly ok: false; readonly error: string };
export interface PairingOffer {
  readonly payload: string;
  readonly code: string;
  readonly address: string;
  readonly expiresIn: number;
}
export type PairingResult = ({ readonly ok: true } & PairingOffer) | Failure;
export interface PhonePage {
  readonly ok: true;
  readonly phones: readonly Phone[];
  readonly next: string | null;
}
export type PhonesResult = PhonePage | Failure;
export type PhoneActionResult = { readonly ok: true } | Failure;
export type PairingStatus = { readonly ok: true; readonly active: boolean } | Failure;
export interface PhoneApi {
  readonly openPhonePairing: () => Promise<PairingResult>;
  readonly closePhonePairing: () => Promise<PhoneActionResult>;
  readonly getPhonePairingStatus: () => Promise<PairingStatus>;
  readonly listPhones: (after: string | null) => Promise<PhonesResult>;
  readonly unpairPhone: (id: string) => Promise<PhoneActionResult>;
}
export const PHONE_OPEN_CHANNEL = 'phone:open';
export const PHONE_CLOSE_CHANNEL = 'phone:close';
export const PHONE_STATUS_CHANNEL = 'phone:status';
export const PHONE_LIST_CHANNEL = 'phone:list';
export const PHONE_UNPAIR_CHANNEL = 'phone:unpair';

function reply(line: string): Record<string, unknown> {
  const value = parseLine(line);
  const error = replyError(value);
  return error === null ? value : { ok: false, error };
}

export function parsePairing(line: string): PairingResult {
  const value = reply(line);
  if (value.ok === false && typeof value.error === 'string')
    return { ok: false, error: value.error };
  if (
    typeof value.payload !== 'string' ||
    typeof value.code !== 'string' ||
    typeof value.address !== 'string' ||
    !isCount(value.expires_in)
  )
    throw new Error('invalid pairing reply');
  return {
    ok: true,
    payload: value.payload,
    code: value.code,
    address: value.address,
    expiresIn: value.expires_in,
  };
}

export function parsePhonePage(line: string): PhonesResult {
  const value = reply(line);
  if (value.ok === false && typeof value.error === 'string')
    return { ok: false, error: value.error };
  if (!Array.isArray(value.phones) || (value.next !== null && typeof value.next !== 'string'))
    throw new Error('invalid phones reply');
  const phones = value.phones.map((phone: unknown): Phone => {
    if (
      !isRecord(phone) ||
      typeof phone.id !== 'string' ||
      typeof phone.name !== 'string' ||
      !isCount(phone.paired_at) ||
      phone.paired_at > 8.64e12
    )
      throw new Error('invalid phone');
    return { id: phone.id, name: phone.name, pairedAt: phone.paired_at };
  });
  return { ok: true, phones, next: value.next };
}

export function parsePhoneStatus(line: string): PairingStatus {
  const value = reply(line);
  if (value.ok === false && typeof value.error === 'string')
    return { ok: false, error: value.error };
  if (typeof value.active !== 'boolean') throw new Error('invalid pairing status reply');
  return { ok: true, active: value.active };
}

export function parsePhoneAck(line: string): PhoneActionResult {
  const value = reply(line);
  if (value.ok === false && typeof value.error === 'string')
    return { ok: false, error: value.error };
  if (value.ok !== true) throw new Error('invalid phone action reply');
  return { ok: true };
}
