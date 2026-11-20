/** Public package entry. Implementation modules are not package subpath APIs. */
export type { VerificationFailureCode, VerificationDiagnostic, VerificationResult } from "./verification.js";
export { AgentSession } from "./agent/session.js";
export { createRegisteredAgentHost } from "./agent/registered-host.js";
export { createRegisteredApplication, createDynamicRegisteredApplication } from "./agent/registered-application.js";
export type {
  RegisteredActionContext, RegisteredControl, RegisteredApplicationObservation, RegisteredApplication,
  DynamicRegisteredApplication,
} from "./agent/registered-application.js";
export type {
  RegisteredApplicationConnection, RegisteredAgentTransport, RegisteredAgentRun,
  RegisteredAgentEndpoint, RegisteredAgentHostOptions, RegisteredAgentHost,
} from "./agent/registered-host.js";
export { AgentWorkflowSession } from "./agent/workflow.js";
export type {
  AgentWorkflowPlan, AgentWorkflowProvider, AgentWorkflowState, AgentWorkflowOutcome,
  AgentWorkflowSessionOptions,
} from "./agent/workflow.js";
export { prepareSemanticWorkflow } from "./agent/workflow-bindings.js";
export type {
  AgentWorkflowBinding, WorkflowResultFormat, WorkflowResultReference, SemanticWorkflowStep,
  WorkflowResultRequirement, AgentWorkflowRequirement, AgentWorkflowRequirementOutcome,
} from "./agent/workflow-bindings.js";
export { prepareSemanticCatalog } from "./semantics/index.js";
export type { SemanticCatalog, SemanticTaskInfo, PreparedSemanticTask, SemanticReportOptions } from "./semantics/index.js";
export { defineSemanticCatalog } from "./semantics/authoring.js";
export { defineRegisteredTask, defineRegisteredTaskV3 } from "./semantics/registered-task.js";
export type {
  RegisteredTaskDefinition, RegisteredTaskDefinitionV3, RegisteredTaskStep, RegisteredReadControlStep,
  RegisteredSetControlTextStep, RegisteredClickControlStep,
} from "./semantics/registered-task.js";
export type {
  SemanticInteger, SemanticScalarType, SemanticLiteral, SemanticOperationSemantics,
  SemanticTaskInput, SemanticTaskArgument, SemanticTerm, SemanticCondition, SemanticTaskStep,
  SemanticReportSegment, SemanticReportTerm, SemanticReportRelation, SemanticReportPredicate,
  SemanticObservedResult, SemanticResultContract, SemanticReportContractV1, SemanticReportContractV2,
  SemanticReportContract, SemanticTaskDescriptionV1, SemanticTaskDescriptionV2, SemanticTaskDescriptionV3, SemanticTaskDescription,
  SemanticDescriptionV1, SemanticDescriptionV2, SemanticDescriptionV3, SemanticDescription,
} from "./semantics/declarations.js";
export { createSemanticProvider } from "./providers/semantic.js";
export type { SemanticProviderOptions } from "./providers/semantic.js";
export { createModelProvider, createModelWorkflowProvider } from "./providers/model.js";
export type { ModelRequest, ModelProviderOptions, ModelWorkflowProviderOptions } from "./providers/model.js";
export type {
  AgentContext, AgentHost, AgentJson, AgentOutcome, AgentPhase, AgentProvider,
  AgentSessionOptions, AgentState, WorkflowPlan,
} from "./agent/session.js";
export { Bridge } from "./bridge.js";
export type { Scope, ControlState, ActionContext, Adapter, Target, Snapshot, Request, Reply } from "./bridge.js";
