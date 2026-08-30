export type PetStatus = "idle" | "chatting" | "exploring" | "returned" | "error";

export interface BootstrapState {
  firstRun: boolean;
  petStatus: PetStatus;
  apiConfigured: boolean;
}
