import React, {
  useState,
  useRef,
  useEffect,
  useId,
  useMemo,
} from 'react';
import { ChevronDown, Check } from 'lucide-react';
import styles from './Select.module.css';

interface Option {
  value: string;
  label: string;
}

interface SelectProps {
  value: string;
  options: Option[];
  onChange: (value: string) => void;
  className?: string;
}

export const Select = ({
  value,
  options,
  onChange,
  className = '',
}: SelectProps) => {
  const [isOpen, setIsOpen] = useState(false);
  const [highlightedIndex, setHighlightedIndex] = useState(-1);
  const containerRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const listboxRef = useRef<HTMLDivElement>(null);
  const listboxId = useId();
  const optionIds = useMemo(
    () => options.map((option) => `${listboxId}-${option.value}`),
    [listboxId, options],
  );

  const selectedOption = options.find((opt) => opt.value === value);
  const selectedIndex = options.findIndex((opt) => opt.value === value);
  const hasOptions = options.length > 0;
  const activeOptionId = highlightedIndex >= 0 ? optionIds[highlightedIndex] : undefined;

  useEffect(() => {
    if (isOpen) {
      setHighlightedIndex(hasOptions ? (selectedIndex >= 0 ? selectedIndex : 0) : -1);
      return;
    }

    setHighlightedIndex(-1);
    triggerRef.current?.focus();
  }, [hasOptions, isOpen, selectedIndex]);

  useEffect(() => {
    if (isOpen) {
      listboxRef.current?.focus();
    }
  }, [isOpen]);

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setIsOpen(false);
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  const handleSelect = (optionValue: string) => {
    onChange(optionValue);
    setIsOpen(false);
  };

  const openDropdown = () => {
    if (!hasOptions) {
      return;
    }

    setIsOpen(true);
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (!hasOptions) {
      if (event.key === 'Enter' || event.key === ' ' || event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault();
      }
      if (event.key === 'Escape') {
        setIsOpen(false);
      }
      return;
    }

    if (!isOpen && (event.key === 'ArrowDown' || event.key === 'ArrowUp')) {
      event.preventDefault();
      openDropdown();
      return;
    }

    if (event.key === 'Escape') {
      setIsOpen(false);
      return;
    }

    if (!isOpen) {
      if (event.key === 'Enter' || event.key === ' ') {
        event.preventDefault();
        openDropdown();
      }
      return;
    }

    if (event.key === 'ArrowDown') {
      event.preventDefault();
      setHighlightedIndex((prev) => (prev + 1) % options.length);
      return;
    }

    if (event.key === 'ArrowUp') {
      event.preventDefault();
      setHighlightedIndex((prev) => (prev - 1 + options.length) % options.length);
      return;
    }

    if ((event.key === 'Enter' || event.key === ' ') && highlightedIndex >= 0) {
      event.preventDefault();
      handleSelect(options[highlightedIndex].value);
    }
  };

  return (
    <div
      ref={containerRef}
      className={`${styles.container} ${className}`}
      onKeyDown={handleKeyDown}
    >
      <button
        ref={triggerRef}
        type="button"
        className={styles.trigger}
        onClick={() => (isOpen ? setIsOpen(false) : openDropdown())}
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        aria-controls={listboxId}
        aria-activedescendant={isOpen ? activeOptionId : undefined}
      >
        <span className={styles.value}>{selectedOption?.label || '请选择'}</span>
        <ChevronDown size={14} className={`${styles.chevron} ${isOpen ? styles.open : ''}`} />
      </button>

      {isOpen && (
        <div
          ref={listboxRef}
          className={styles.dropdown}
          role="listbox"
          id={listboxId}
          tabIndex={-1}
          aria-activedescendant={activeOptionId}
        >
          {options.map((option, index) => (
            <button
              key={option.value}
              id={optionIds[index]}
              type="button"
              role="option"
              aria-selected={option.value === value}
              className={`${styles.option} ${option.value === value ? styles.selected : ''}`}
              onClick={() => handleSelect(option.value)}
              onMouseEnter={() => setHighlightedIndex(index)}
              tabIndex={-1}
            >
              <span>{option.label}</span>
              {option.value === value && <Check size={14} />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
};
