import { useCallback, useEffect, useState } from 'react';
import type { RefCallback } from 'react';

export interface Size {
  readonly width: number;
  readonly height: number;
}

/**
 * The size of an element, which follows it as the window or the layout around it changes. The
 * size is 0 by 0 until the element is there and has been measured.
 */
export function useSize<T extends HTMLElement>(): [RefCallback<T>, Size] {
  const [element, setElement] = useState<T | null>(null);
  const [size, setSize] = useState<Size>({ width: 0, height: 0 });
  const ref = useCallback((node: T | null) => {
    setElement(node);
  }, []);
  useEffect(() => {
    if (element === null) {
      return;
    }
    const measure = (): void => {
      const { clientWidth: width, clientHeight: height } = element;
      setSize((current) =>
        current.width === width && current.height === height ? current : { width, height },
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, [element]);
  return [ref, size];
}
