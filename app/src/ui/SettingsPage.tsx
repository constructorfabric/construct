import { Heading } from '@gears-frontx/ui-kit';
import type { ReactNode } from 'react';
import { t } from '../translation';

export function SettingsPage(): ReactNode {
  return (
    <Heading typography="display" render={<h1 />}>
      {t('SettingsPage.Title')}
    </Heading>
  );
}
