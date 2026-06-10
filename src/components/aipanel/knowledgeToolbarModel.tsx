export interface KnowledgeToolbarAction {
  label: string;
  onClick: () => void | Promise<void>;
  disabled?: boolean;
  icon?: React.ReactNode;
}

export function buildKnowledgeToolbarModel() {
  // Knowledge base management buttons have been removed from the AI panel toolbar.
  // Users now manage knowledge base membership from the sidebar's knowledge select mode.
  // The toolbar shows only a status label in knowledge mode.
  return {
    primaryAction: null,
    secondaryAction: null,
  };
}
