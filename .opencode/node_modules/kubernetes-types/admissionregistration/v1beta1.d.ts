import { Condition, LabelSelector, ListMeta, ObjectMeta } from "../meta/v1";
/** AuditAnnotation describes how to produce an audit annotation for an API request. */
export interface AuditAnnotation {
    /**
     * key specifies the audit annotation key. The audit annotation keys of a ValidatingAdmissionPolicy must be unique. The key must be a qualified name ([A-Za-z0-9][-A-Za-z0-9_.]*) no more than 63 bytes in length.
     *
     * The key is combined with the resource name of the ValidatingAdmissionPolicy to construct an audit annotation key: "{ValidatingAdmissionPolicy name}/{key}".
     *
     * If an admission webhook uses the same resource name as this ValidatingAdmissionPolicy and the same audit annotation key, the annotation key will be identical. In this case, the first annotation written with the key will be included in the audit event and all subsequent annotations with the same key will be discarded.
     *
     * Required.
     */
    key: string;
    /**
     * valueExpression represents the expression which is evaluated by CEL to produce an audit annotation value. The expression must evaluate to either a string or null value. If the expression evaluates to a string, the audit annotation is included with the string value. If the expression evaluates to null or empty string the audit annotation will be omitted. The valueExpression may be no longer than 5kb in length. If the result of the valueExpression is more than 10kb in length, it will be truncated to 10kb.
     *
     * If multiple ValidatingAdmissionPolicyBinding resources match an API request, then the valueExpression will be evaluated for each binding. All unique values produced by the valueExpressions will be joined together in a comma-separated list.
     *
     * Required.
     */
    valueExpression: string;
}
/** ExpressionWarning is a warning information that targets a specific expression. */
export interface ExpressionWarning {
    /** The path to the field that refers the expression. For example, the reference to the expression of the first item of validations is "spec.validations[0].expression" */
    fieldRef: string;
    /** The content of type checking information in a human-readable form. Each line of the warning contains the type that the expression is checked against, followed by the type check error from the compiler. */
    warning: string;
}
/** MatchCondition represents a condition which must be fulfilled for a request to be sent to a webhook. */
export interface MatchCondition {
    /**
     * Expression represents the expression which will be evaluated by CEL. Must evaluate to bool. CEL expressions have access to the contents of the AdmissionRequest and Authorizer, organized into CEL variables:
     *
     * 'object' - The object from the incoming request. The value is null for DELETE requests. 'oldObject' - The existing object. The value is null for CREATE requests. 'request' - Attributes of the admission request(/pkg/apis/admission/types.go#AdmissionRequest). 'authorizer' - A CEL Authorizer. May be used to perform authorization checks for the principal (user or service account) of the request.
     *   See https://pkg.go.dev/k8s.io/apiserver/pkg/cel/library#Authz
     * 'authorizer.requestResource' - A CEL ResourceCheck constructed from the 'authorizer' and configured with the
     *   request resource.
     * Documentation on CEL: https://kubernetes.io/docs/reference/using-api/cel/
     *
     * Required.
     */
    expression: string;
    /**
     * Name is an identifier for this match condition, used for strategic merging of MatchConditions, as well as providing an identifier for logging purposes. A good name should be descriptive of the associated expression. Name must be a qualified name consisting of alphanumeric characters, '-', '_' or '.', and must start and end with an alphanumeric character (e.g. 'MyName',  or 'my.name',  or '123-abc', regex used for validation is '([A-Za-z0-9][-A-Za-z0-9_.]*)?[A-Za-z0-9]') with an optional DNS subdomain prefix and '/' (e.g. 'example.com/MyName')
     *
     * Required.
     */
    name: string;
}
/** MatchResources decides whether to run the admission control policy on an object based on whether it meets the match criteria. The exclude rules take precedence over include rules (if a resource matches both, it is excluded) */
export interface MatchResources {
    /** ExcludeResourceRules describes what operations on what resources/subresources the ValidatingAdmissionPolicy should not care about. The exclude rules take precedence over include rules (if a resource matches both, it is excluded) */
    excludeResourceRules?: Array<NamedRuleWithOperations>;
    /**
     * matchPolicy defines how the "MatchResources" list is used to match incoming requests. Allowed values are "Exact" or "Equivalent".
     *
     * - Exact: match a request only if it exactly matches a specified rule. For example, if deployments can be modified via apps/v1, apps/v1beta1, and extensions/v1beta1, but "rules" only included `apiGroups:["apps"], apiVersions:["v1"], resources: ["deployments"]`, a request to apps/v1beta1 or extensions/v1beta1 would not be sent to the ValidatingAdmissionPolicy.
     *
     * - Equivalent: match a request if modifies a resource listed in rules, even via another API group or version. For example, if deployments can be modified via apps/v1, apps/v1beta1, and extensions/v1beta1, and "rules" only included `apiGroups:["apps"], apiVersions:["v1"], resources: ["deployments"]`, a request to apps/v1beta1 or extensions/v1beta1 would be converted to apps/v1 and sent to the ValidatingAdmissionPolicy.
     *
     * Defaults to "Equivalent"
     */
    matchPolicy?: string;
    /**
     * NamespaceSelector decides whether to run the admission control policy on an object based on whether the namespace for that object matches the selector. If the object itself is a namespace, the matching is performed on object.metadata.labels. If the object is another cluster scoped resource, it never skips the policy.
     *
     * For example, to run the webhook on any objects whose namespace is not associated with "runlevel" of "0" or "1";  you will set the selector as follows: "namespaceSelector": {
     *   "matchExpressions": [
     *     {
     *       "key": "runlevel",
     *       "operator": "NotIn",
     *       "values": [
     *         "0",
     *         "1"
     *       ]
     *     }
     *   ]
     * }
     *
     * If instead you want to only run the policy on any objects whose namespace is associated with the "environment" of "prod" or "staging"; you will set the selector as follows: "namespaceSelector": {
     *   "matchExpressions": [
     *     {
     *       "key": "environment",
     *       "operator": "In",
     *       "values": [
     *         "prod",
     *         "staging"
     *       ]
     *     }
     *   ]
     * }
     *
     * See https://kubernetes.io/docs/concepts/overview/working-with-objects/labels/ for more examples of label selectors.
     *
     * Default to the empty LabelSelector, which matches everything.
     */
    namespaceSelector?: LabelSelector;
    /** ObjectSelector decides whether to run the validation based on if the object has matching labels. objectSelector is evaluated against both the oldObject and newObject that would be sent to the cel validation, and is considered to match if either object matches the selector. A null object (oldObject in the case of create, or newObject in the case of delete) or an object that cannot have labels (like a DeploymentRollback or a PodProxyOptions object) is not considered to match. Use the object selector only if the webhook is opt-in, because end users may skip the admission webhook by setting the labels. Default to the empty LabelSelector, which matches everything. */
    objectSelector?: LabelSelector;
    /** ResourceRules describes what operations on what resources/subresources the ValidatingAdmissionPolicy matches. The policy cares about an operation if it matches _any_ Rule. */
    resourceRules?: Array<NamedRuleWithOperations>;
}
/** NamedRuleWithOperations is a tuple of Operations and Resources with ResourceNames. */
export interface NamedRuleWithOperations {
    /** APIGroups is the API groups the resources belong to. '*' is all groups. If '*' is present, the length of the slice must be one. Required. */
    apiGroups?: Array<string>;
    /** APIVersions is the API versions the resources belong to. '*' is all versions. If '*' is present, the length of the slice must be one. Required. */
    apiVersions?: Array<string>;
    /** Operations is the operations the admission hook cares about - CREATE, UPDATE, DELETE, CONNECT or * for all of those operations and any future admission operations that are added. If '*' is present, the length of the slice must be one. Required. */
    operations?: Array<string>;
    /** ResourceNames is an optional white list of names that the rule applies to.  An empty set means that everything is allowed. */
    resourceNames?: Array<string>;
}
/** ParamKind is a tuple of Group Kind and Version. */
export interface ParamKind {
    /** APIVersion is the API group version the resources belong to. In format of "group/version". Required. */
    apiVersion?: string;
    /** Kind is the API kind the resources belong to. Required. */
    kind?: string;
}
/** ParamRef describes how to locate the params to be used as input to expressions of rules applied by a policy binding. */
export interface ParamRef {
    /**
     * name is the name of the resource being referenced.
     *
     * One of `name` or `selector` must be set, but `name` and `selector` are mutually exclusive properties. If one is set, the other must be unset.
     *
     * A single parameter used for all admission requests can be configured by setting the `name` field, leaving `selector` blank, and setting namespace if `paramKind` is namespace-scoped.
     */
    name?: string;
    /**
     * namespace is the namespace of the referenced resource. Allows limiting the search for params to a specific namespace. Applies to both `name` and `selector` fields.
     *
     * A per-namespace parameter may be used by specifying a namespace-scoped `paramKind` in the policy and leaving this field empty.
     *
     * - If `paramKind` is cluster-scoped, this field MUST be unset. Setting this field results in a configuration error.
     *
     * - If `paramKind` is namespace-scoped, the namespace of the object being evaluated for admission will be used when this field is left unset. Take care that if this is left empty the binding must not match any cluster-scoped resources, which will result in an error.
     */
    namespace?: string;
    /**
     * `parameterNotFoundAction` controls the behavior of the binding when the resource exists, and name or selector is valid, but there are no parameters matched by the binding. If the value is set to `Allow`, then no matched parameters will be treated as successful validation by the binding. If set to `Deny`, then no matched parameters will be subject to the `failurePolicy` of the policy.
     *
     * Allowed values are `Allow` or `Deny`
     *
     * Required
     */
    parameterNotFoundAction?: string;
    /**
     * selector can be used to match multiple param objects based on their labels. Supply selector: {} to match all resources of the ParamKind.
     *
     * If multiple params are found, they are all evaluated with the policy expressions and the results are ANDed together.
     *
     * One of `name` or `selector` must be set, but `name` and `selector` are mutually exclusive properties. If one is set, the other must be unset.
     */
    selector?: LabelSelector;
}
/** TypeChecking contains results of type checking the expressions in the ValidatingAdmissionPolicy */
export interface TypeChecking {
    /** The type checking warnings for each expression. */
    expressionWarnings?: Array<ExpressionWarning>;
}
/** ValidatingAdmissionPolicy describes the definition of an admission validation policy that accepts or rejects an object without changing it. */
export interface ValidatingAdmissionPolicy {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "admissionregistration.k8s.io/v1beta1";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ValidatingAdmissionPolicy";
    /** Standard object metadata; More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata. */
    metadata?: ObjectMeta;
    /** Specification of the desired behavior of the ValidatingAdmissionPolicy. */
    spec?: ValidatingAdmissionPolicySpec;
    /** The status of the ValidatingAdmissionPolicy, including warnings that are useful to determine if the policy behaves in the expected way. Populated by the system. Read-only. */
    readonly status?: ValidatingAdmissionPolicyStatus;
}
/**
 * ValidatingAdmissionPolicyBinding binds the ValidatingAdmissionPolicy with paramerized resources. ValidatingAdmissionPolicyBinding and parameter CRDs together define how cluster administrators configure policies for clusters.
 *
 * For a given admission request, each binding will cause its policy to be evaluated N times, where N is 1 for policies/bindings that don't use params, otherwise N is the number of parameters selected by the binding.
 *
 * The CEL expressions of a policy must have a computed CEL cost below the maximum CEL budget. Each evaluation of the policy is given an independent CEL cost budget. Adding/removing policies, bindings, or params can not affect whether a given (policy, binding, param) combination is within its own CEL budget.
 */
export interface ValidatingAdmissionPolicyBinding {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "admissionregistration.k8s.io/v1beta1";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ValidatingAdmissionPolicyBinding";
    /** Standard object metadata; More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata. */
    metadata?: ObjectMeta;
    /** Specification of the desired behavior of the ValidatingAdmissionPolicyBinding. */
    spec?: ValidatingAdmissionPolicyBindingSpec;
}
/** ValidatingAdmissionPolicyBindingList is a list of ValidatingAdmissionPolicyBinding. */
export interface ValidatingAdmissionPolicyBindingList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "admissionregistration.k8s.io/v1beta1";
    /** List of PolicyBinding. */
    items?: Array<ValidatingAdmissionPolicyBinding>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ValidatingAdmissionPolicyBindingList";
    /** Standard list metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    metadata?: ListMeta;
}
/** ValidatingAdmissionPolicyBindingSpec is the specification of the ValidatingAdmissionPolicyBinding. */
export interface ValidatingAdmissionPolicyBindingSpec {
    /** MatchResources declares what resources match this binding and will be validated by it. Note that this is intersected with the policy's matchConstraints, so only requests that are matched by the policy can be selected by this. If this is unset, all resources matched by the policy are validated by this binding When resourceRules is unset, it does not constrain resource matching. If a resource is matched by the other fields of this object, it will be validated. Note that this is differs from ValidatingAdmissionPolicy matchConstraints, where resourceRules are required. */
    matchResources?: MatchResources;
    /** paramRef specifies the parameter resource used to configure the admission control policy. It should point to a resource of the type specified in ParamKind of the bound ValidatingAdmissionPolicy. If the policy specifies a ParamKind and the resource referred to by ParamRef does not exist, this binding is considered mis-configured and the FailurePolicy of the ValidatingAdmissionPolicy applied. If the policy does not specify a ParamKind then this field is ignored, and the rules are evaluated without a param. */
    paramRef?: ParamRef;
    /** PolicyName references a ValidatingAdmissionPolicy name which the ValidatingAdmissionPolicyBinding binds to. If the referenced resource does not exist, this binding is considered invalid and will be ignored Required. */
    policyName?: string;
    /**
     * validationActions declares how Validations of the referenced ValidatingAdmissionPolicy are enforced. If a validation evaluates to false it is always enforced according to these actions.
     *
     * Failures defined by the ValidatingAdmissionPolicy's FailurePolicy are enforced according to these actions only if the FailurePolicy is set to Fail, otherwise the failures are ignored. This includes compilation errors, runtime errors and misconfigurations of the policy.
     *
     * validationActions is declared as a set of action values. Order does not matter. validationActions may not contain duplicates of the same action.
     *
     * The supported actions values are:
     *
     * "Deny" specifies that a validation failure results in a denied request.
     *
     * "Warn" specifies that a validation failure is reported to the request client in HTTP Warning headers, with a warning code of 299. Warnings can be sent both for allowed or denied admission responses.
     *
     * "Audit" specifies that a validation failure is included in the published audit event for the request. The audit event will contain a `validation.policy.admission.k8s.io/validation_failure` audit annotation with a value containing the details of the validation failures, formatted as a JSON list of objects, each with the following fields: - message: The validation failure message string - policy: The resource name of the ValidatingAdmissionPolicy - binding: The resource name of the ValidatingAdmissionPolicyBinding - expressionIndex: The index of the failed validations in the ValidatingAdmissionPolicy - validationActions: The enforcement actions enacted for the validation failure Example audit annotation: `"validation.policy.admission.k8s.io/validation_failure": "[{"message": "Invalid value", {"policy": "policy.example.com", {"binding": "policybinding.example.com", {"expressionIndex": "1", {"validationActions": ["Audit"]}]"`
     *
     * Clients should expect to handle additional values by ignoring any values not recognized.
     *
     * "Deny" and "Warn" may not be used together since this combination needlessly duplicates the validation failure both in the API response body and the HTTP warning headers.
     *
     * Required.
     */
    validationActions?: Array<string>;
}
/** ValidatingAdmissionPolicyList is a list of ValidatingAdmissionPolicy. */
export interface ValidatingAdmissionPolicyList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "admissionregistration.k8s.io/v1beta1";
    /** List of ValidatingAdmissionPolicy. */
    items?: Array<ValidatingAdmissionPolicy>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ValidatingAdmissionPolicyList";
    /** Standard list metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    metadata?: ListMeta;
}
/** ValidatingAdmissionPolicySpec is the specification of the desired behavior of the AdmissionPolicy. */
export interface ValidatingAdmissionPolicySpec {
    /** auditAnnotations contains CEL expressions which are used to produce audit annotations for the audit event of the API request. validations and auditAnnotations may not both be empty; a least one of validations or auditAnnotations is required. */
    auditAnnotations?: Array<AuditAnnotation>;
    /**
     * failurePolicy defines how to handle failures for the admission policy. Failures can occur from CEL expression parse errors, type check errors, runtime errors and invalid or mis-configured policy definitions or bindings.
     *
     * A policy is invalid if spec.paramKind refers to a non-existent Kind. A binding is invalid if spec.paramRef.name refers to a non-existent resource.
     *
     * failurePolicy does not define how validations that evaluate to false are handled.
     *
     * When failurePolicy is set to Fail, ValidatingAdmissionPolicyBinding validationActions define how failures are enforced.
     *
     * Allowed values are Ignore or Fail. Defaults to Fail.
     */
    failurePolicy?: string;
    /**
     * MatchConditions is a list of conditions that must be met for a request to be validated. Match conditions filter requests that have already been matched by the rules, namespaceSelector, and objectSelector. An empty list of matchConditions matches all requests. There are a maximum of 64 match conditions allowed.
     *
     * If a parameter object is provided, it can be accessed via the `params` handle in the same manner as validation expressions.
     *
     * The exact matching logic is (in order):
     *   1. If ANY matchCondition evaluates to FALSE, the policy is skipped.
     *   2. If ALL matchConditions evaluate to TRUE, the policy is evaluated.
     *   3. If any matchCondition evaluates to an error (but none are FALSE):
     *      - If failurePolicy=Fail, reject the request
     *      - If failurePolicy=Ignore, the policy is skipped
     */
    matchConditions?: Array<MatchCondition>;
    /** MatchConstraints specifies what resources this policy is designed to validate. The AdmissionPolicy cares about a request if it matches _all_ Constraints. However, in order to prevent clusters from being put into an unstable state that cannot be recovered from via the API ValidatingAdmissionPolicy cannot match ValidatingAdmissionPolicy and ValidatingAdmissionPolicyBinding. Required. */
    matchConstraints?: MatchResources;
    /** ParamKind specifies the kind of resources used to parameterize this policy. If absent, there are no parameters for this policy and the param CEL variable will not be provided to validation expressions. If ParamKind refers to a non-existent kind, this policy definition is mis-configured and the FailurePolicy is applied. If paramKind is specified but paramRef is unset in ValidatingAdmissionPolicyBinding, the params variable will be null. */
    paramKind?: ParamKind;
    /** Validations contain CEL expressions which is used to apply the validation. Validations and AuditAnnotations may not both be empty; a minimum of one Validations or AuditAnnotations is required. */
    validations?: Array<Validation>;
    /**
     * Variables contain definitions of variables that can be used in composition of other expressions. Each variable is defined as a named CEL expression. The variables defined here will be available under `variables` in other expressions of the policy except MatchConditions because MatchConditions are evaluated before the rest of the policy.
     *
     * The expression of a variable can refer to other variables defined earlier in the list but not those after. Thus, Variables must be sorted by the order of first appearance and acyclic.
     */
    variables?: Array<Variable>;
}
/** ValidatingAdmissionPolicyStatus represents the status of an admission validation policy. */
export interface ValidatingAdmissionPolicyStatus {
    /** The conditions represent the latest available observations of a policy's current state. */
    conditions?: Array<Condition>;
    /** The generation observed by the controller. */
    observedGeneration?: number;
    /** The results of type checking for each expression. Presence of this field indicates the completion of the type checking. */
    typeChecking?: TypeChecking;
}
/** Validation specifies the CEL expression which is used to apply the validation. */
export interface Validation {
    /**
     * Expression represents the expression which will be evaluated by CEL. ref: https://github.com/google/cel-spec CEL expressions have access to the contents of the API request/response, organized into CEL variables as well as some other useful variables:
     *
     * - 'object' - The object from the incoming request. The value is null for DELETE requests. - 'oldObject' - The existing object. The value is null for CREATE requests. - 'request' - Attributes of the API request([ref](/pkg/apis/admission/types.go#AdmissionRequest)). - 'params' - Parameter resource referred to by the policy binding being evaluated. Only populated if the policy has a ParamKind. - 'namespaceObject' - The namespace object that the incoming object belongs to. The value is null for cluster-scoped resources. - 'variables' - Map of composited variables, from its name to its lazily evaluated value.
     *   For example, a variable named 'foo' can be accessed as 'variables.foo'.
     * - 'authorizer' - A CEL Authorizer. May be used to perform authorization checks for the principal (user or service account) of the request.
     *   See https://pkg.go.dev/k8s.io/apiserver/pkg/cel/library#Authz
     * - 'authorizer.requestResource' - A CEL ResourceCheck constructed from the 'authorizer' and configured with the
     *   request resource.
     *
     * The `apiVersion`, `kind`, `metadata.name` and `metadata.generateName` are always accessible from the root of the object. No other metadata properties are accessible.
     *
     * Only property names of the form `[a-zA-Z_.-/][a-zA-Z0-9_.-/]*` are accessible. Accessible property names are escaped according to the following rules when accessed in the expression: - '__' escapes to '__underscores__' - '.' escapes to '__dot__' - '-' escapes to '__dash__' - '/' escapes to '__slash__' - Property names that exactly match a CEL RESERVED keyword escape to '__{keyword}__'. The keywords are:
     * 	  "true", "false", "null", "in", "as", "break", "const", "continue", "else", "for", "function", "if",
     * 	  "import", "let", "loop", "package", "namespace", "return".
     * Examples:
     *   - Expression accessing a property named "namespace": {"Expression": "object.__namespace__ > 0"}
     *   - Expression accessing a property named "x-prop": {"Expression": "object.x__dash__prop > 0"}
     *   - Expression accessing a property named "redact__d": {"Expression": "object.redact__underscores__d > 0"}
     *
     * Equality on arrays with list type of 'set' or 'map' ignores element order, i.e. [1, 2] == [2, 1]. Concatenation on arrays with x-kubernetes-list-type use the semantics of the list type:
     *   - 'set': `X + Y` performs a union where the array positions of all elements in `X` are preserved and
     *     non-intersecting elements in `Y` are appended, retaining their partial order.
     *   - 'map': `X + Y` performs a merge where the array positions of all keys in `X` are preserved but the values
     *     are overwritten by values in `Y` when the key sets of `X` and `Y` intersect. Elements in `Y` with
     *     non-intersecting keys are appended, retaining their partial order.
     * Required.
     */
    expression: string;
    /** Message represents the message displayed when validation fails. The message is required if the Expression contains line breaks. The message must not contain line breaks. If unset, the message is "failed rule: {Rule}". e.g. "must be a URL with the host matching spec.host" If the Expression contains line breaks. Message is required. The message must not contain line breaks. If unset, the message is "failed Expression: {Expression}". */
    message?: string;
    /** messageExpression declares a CEL expression that evaluates to the validation failure message that is returned when this rule fails. Since messageExpression is used as a failure message, it must evaluate to a string. If both message and messageExpression are present on a validation, then messageExpression will be used if validation fails. If messageExpression results in a runtime error, the runtime error is logged, and the validation failure message is produced as if the messageExpression field were unset. If messageExpression evaluates to an empty string, a string with only spaces, or a string that contains line breaks, then the validation failure message will also be produced as if the messageExpression field were unset, and the fact that messageExpression produced an empty string/string with only spaces/string with line breaks will be logged. messageExpression has access to all the same variables as the `expression` except for 'authorizer' and 'authorizer.requestResource'. Example: "object.x must be less than max ("+string(params.max)+")" */
    messageExpression?: string;
    /** Reason represents a machine-readable description of why this validation failed. If this is the first validation in the list to fail, this reason, as well as the corresponding HTTP response code, are used in the HTTP response to the client. The currently supported reasons are: "Unauthorized", "Forbidden", "Invalid", "RequestEntityTooLarge". If not set, StatusReasonInvalid is used in the response to the client. */
    reason?: string;
}
/** Variable is the definition of a variable that is used for composition. A variable is defined as a named expression. */
export interface Variable {
    /** Expression is the expression that will be evaluated as the value of the variable. The CEL expression has access to the same identifiers as the CEL expressions in Validation. */
    expression: string;
    /** Name is the name of the variable. The name must be a valid CEL identifier and unique among all variables. The variable can be accessed in other expressions through `variables` For example, if name is "foo", the variable will be available as `variables.foo` */
    name: string;
}
