import { ObjectMeta } from "../meta/v1";
/** LocalSubjectAccessReview checks whether or not a user or group can perform an action in a given namespace. Having a namespace scoped resource makes it much easier to grant namespace scoped policy that includes permissions checking. */
export interface LocalSubjectAccessReview {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "authorization.k8s.io/v1";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "LocalSubjectAccessReview";
    /** Standard list metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ObjectMeta;
    /** Spec holds information about the request being evaluated.  spec.namespace must be equal to the namespace you made the request against.  If empty, it is defaulted. */
    spec: SubjectAccessReviewSpec;
    /** Status is filled in by the server and indicates whether the request is allowed or not */
    status?: SubjectAccessReviewStatus;
}
/** NonResourceAttributes includes the authorization attributes available for non-resource requests to the Authorizer interface */
export interface NonResourceAttributes {
    /** Path is the URL path of the request */
    path?: string;
    /** Verb is the standard HTTP verb */
    verb?: string;
}
/** NonResourceRule holds information that describes a rule for the non-resource */
export interface NonResourceRule {
    /** NonResourceURLs is a set of partial urls that a user should have access to.  *s are allowed, but only as the full, final step in the path.  "*" means all. */
    nonResourceURLs?: Array<string>;
    /** Verb is a list of kubernetes non-resource API verbs, like: get, post, put, delete, patch, head, options.  "*" means all. */
    verbs: Array<string>;
}
/** ResourceAttributes includes the authorization attributes available for resource requests to the Authorizer interface */
export interface ResourceAttributes {
    /** Group is the API Group of the Resource.  "*" means all. */
    group?: string;
    /** Name is the name of the resource being requested for a "get" or deleted for a "delete". "" (empty) means all. */
    name?: string;
    /** Namespace is the namespace of the action being requested.  Currently, there is no distinction between no namespace and all namespaces "" (empty) is defaulted for LocalSubjectAccessReviews "" (empty) is empty for cluster-scoped resources "" (empty) means "all" for namespace scoped resources from a SubjectAccessReview or SelfSubjectAccessReview */
    namespace?: string;
    /** Resource is one of the existing resource types.  "*" means all. */
    resource?: string;
    /** Subresource is one of the existing resource types.  "" means none. */
    subresource?: string;
    /** Verb is a kubernetes resource API verb, like: get, list, watch, create, update, delete, proxy.  "*" means all. */
    verb?: string;
    /** Version is the API Version of the Resource.  "*" means all. */
    version?: string;
}
/** ResourceRule is the list of actions the subject is allowed to perform on resources. The list ordering isn't significant, may contain duplicates, and possibly be incomplete. */
export interface ResourceRule {
    /** APIGroups is the name of the APIGroup that contains the resources.  If multiple API groups are specified, any action requested against one of the enumerated resources in any API group will be allowed.  "*" means all. */
    apiGroups?: Array<string>;
    /** ResourceNames is an optional white list of names that the rule applies to.  An empty set means that everything is allowed.  "*" means all. */
    resourceNames?: Array<string>;
}
/** SelfSubjectAccessReview checks whether or the current user can perform an action.  Not filling in a spec.namespace means "in all namespaces".  Self is a special case, because users should always be able to check whether they can perform an action */
export interface SelfSubjectAccessReview {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "authorization.k8s.io/v1";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "SelfSubjectAccessReview";
    /** Standard list metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ObjectMeta;
    /** Spec holds information about the request being evaluated.  user and groups must be empty */
    spec: SelfSubjectAccessReviewSpec;
    /** Status is filled in by the server and indicates whether the request is allowed or not */
    status?: SubjectAccessReviewStatus;
}
/** SelfSubjectAccessReviewSpec is a description of the access request.  Exactly one of ResourceAuthorizationAttributes and NonResourceAuthorizationAttributes must be set */
export interface SelfSubjectAccessReviewSpec {
    /** NonResourceAttributes describes information for a non-resource access request */
    nonResourceAttributes?: NonResourceAttributes;
    /** ResourceAuthorizationAttributes describes information for a resource access request */
    resourceAttributes?: ResourceAttributes;
}
/** SelfSubjectRulesReview enumerates the set of actions the current user can perform within a namespace. The returned list of actions may be incomplete depending on the server's authorization mode, and any errors experienced during the evaluation. SelfSubjectRulesReview should be used by UIs to show/hide actions, or to quickly let an end user reason about their permissions. It should NOT Be used by external systems to drive authorization decisions as this raises confused deputy, cache lifetime/revocation, and correctness concerns. SubjectAccessReview, and LocalAccessReview are the correct way to defer authorization decisions to the API server. */
export interface SelfSubjectRulesReview {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "authorization.k8s.io/v1";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "SelfSubjectRulesReview";
    /** Standard list metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ObjectMeta;
    /** Spec holds information about the request being evaluated. */
    spec: SelfSubjectRulesReviewSpec;
    /** Status is filled in by the server and indicates the set of actions a user can perform. */
    status?: SubjectRulesReviewStatus;
}
/** SelfSubjectRulesReviewSpec defines the specification for SelfSubjectRulesReview. */
export interface SelfSubjectRulesReviewSpec {
    /** Namespace to evaluate rules for. Required. */
    namespace?: string;
}
/** SubjectAccessReview checks whether or not a user or group can perform an action. */
export interface SubjectAccessReview {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "authorization.k8s.io/v1";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "SubjectAccessReview";
    /** Standard list metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ObjectMeta;
    /** Spec holds information about the request being evaluated */
    spec: SubjectAccessReviewSpec;
    /** Status is filled in by the server and indicates whether the request is allowed or not */
    status?: SubjectAccessReviewStatus;
}
/** SubjectAccessReviewSpec is a description of the access request.  Exactly one of ResourceAuthorizationAttributes and NonResourceAuthorizationAttributes must be set */
export interface SubjectAccessReviewSpec {
    /** Extra corresponds to the user.Info.GetExtra() method from the authenticator.  Since that is input to the authorizer it needs a reflection here. */
    extra?: {
        [name: string]: Array<string>;
    };
    /** Groups is the groups you're testing for. */
    groups?: Array<string>;
    /** NonResourceAttributes describes information for a non-resource access request */
    nonResourceAttributes?: NonResourceAttributes;
    /** ResourceAuthorizationAttributes describes information for a resource access request */
    resourceAttributes?: ResourceAttributes;
    /** UID information about the requesting user. */
    uid?: string;
    /** User is the user you're testing for. If you specify "User" but not "Groups", then is it interpreted as "What if User were not a member of any groups */
    user?: string;
}
/** SubjectAccessReviewStatus */
export interface SubjectAccessReviewStatus {
    /** Allowed is required. True if the action would be allowed, false otherwise. */
    allowed: boolean;
    /** Denied is optional. True if the action would be denied, otherwise false. If both allowed is false and denied is false, then the authorizer has no opinion on whether to authorize the action. Denied may not be true if Allowed is true. */
    denied?: boolean;
    /** EvaluationError is an indication that some error occurred during the authorization check. It is entirely possible to get an error and be able to continue determine authorization status in spite of it. For instance, RBAC can be missing a role, but enough roles are still present and bound to reason about the request. */
    evaluationError?: string;
    /** Reason is optional.  It indicates why a request was allowed or denied. */
    reason?: string;
}
/** SubjectRulesReviewStatus contains the result of a rules check. This check can be incomplete depending on the set of authorizers the server is configured with and any errors experienced during evaluation. Because authorization rules are additive, if a rule appears in a list it's safe to assume the subject has that permission, even if that list is incomplete. */
export interface SubjectRulesReviewStatus {
    /** EvaluationError can appear in combination with Rules. It indicates an error occurred during rule evaluation, such as an authorizer that doesn't support rule evaluation, and that ResourceRules and/or NonResourceRules may be incomplete. */
    evaluationError?: string;
    /** Incomplete is true when the rules returned by this call are incomplete. This is most commonly encountered when an authorizer, such as an external authorizer, doesn't support rules evaluation. */
    incomplete: boolean;
    /** NonResourceRules is the list of actions the subject is allowed to perform on non-resources. The list ordering isn't significant, may contain duplicates, and possibly be incomplete. */
    nonResourceRules: Array<NonResourceRule>;
    /** ResourceRules is the list of actions the subject is allowed to perform on resources. The list ordering isn't significant, may contain duplicates, and possibly be incomplete. */
    resourceRules: Array<ResourceRule>;
}
