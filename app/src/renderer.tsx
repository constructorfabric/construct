import '@gears-frontx/ui-kit/theme.css';
import '@gears-frontx/ui-kit/typeset.css';
import { CSPProvider } from '@base-ui/react/csp-provider';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './ui/App';

const root = document.getElementById('root');
if (root === null) {
  throw new Error('the window markup has no #root');
}

// The page's policy allows no `<style>` element, which Base UI would add to its popups and
// scroll areas.
createRoot(root).render(
  <StrictMode>
    <CSPProvider disableStyleElements>
      <App />
    </CSPProvider>
  </StrictMode>,
);
