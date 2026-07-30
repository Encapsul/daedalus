import { NodeSelector } from "../core/v1";
import { RawExtension } from "../runtime";
import { Quantity } from "../api/resource";
import { ListMeta, ObjectMeta } from "../meta/v1";
/** AllocationResult contains attributes of an allocated resource. */
export interface AllocationResult {
    /**
     * This field will get set by the resource driver after it has allocated the resource to inform the scheduler where it can schedule Pods using the ResourceClaim.
     *
     * Setting this field is optional. If null, the resource is available everywhere.
     */
    availableOnNodes?: NodeSelector;
    /**
     * ResourceHandles contain the state associated with an allocation that should be maintained throughout the lifetime of a claim. Each ResourceHandle contains data that should be passed to a specific kubelet plugin once it lands on a node. This data is returned by the driver after a successful allocation and is opaque to Kubernetes. Driver documentation may explain to users how to interpret this data if needed.
     *
     * Setting this field is optional. It has a maximum size of 32 entries. If null (or empty), it is assumed this allocation will be processed by a single kubelet plugin with no ResourceHandle data attached. The name of the kubelet plugin invoked will match the DriverName set in the ResourceClaimStatus this AllocationResult is embedded in.
     */
    resourceHandles?: Array<ResourceHandle>;
    /** Shareable determines whether the resource supports more than one consumer at a time. */
    shareable?: boolean;
}
/** DriverAllocationResult contains vendor parameters and the allocation result for one request. */
export interface DriverAllocationResult {
    /** NamedResources describes the allocation result when using the named resources model. */
    namedResources?: NamedResourcesAllocationResult;
    /** VendorRequestParameters are the per-request configuration parameters from the time that the claim was allocated. */
    vendorRequestParameters?: RawExtension;
}
/** DriverRequests describes all resources that are needed from one particular driver. */
export interface DriverRequests {
    /** DriverName is the name used by the DRA driver kubelet plugin. */
    driverName?: string;
    /** Requests describes all resources that are needed from the driver. */
    requests?: Array<ResourceRequest>;
    /** VendorParameters are arbitrary setup parameters for all requests of the claim. They are ignored while allocating the claim. */
    vendorParameters?: RawExtension;
}
/** NamedResourcesAllocationResult is used in AllocationResultModel. */
export interface NamedResourcesAllocationResult {
    /** Name is the name of the selected resource instance. */
    name: string;
}
/** NamedResourcesAttribute is a combination of an attribute name and its value. */
export interface NamedResourcesAttribute {
    /** BoolValue is a true/false value. */
    bool?: boolean;
    /** IntValue is a 64-bit integer. */
    int?: number;
    /** IntSliceValue is an array of 64-bit integers. */
    intSlice?: NamedResourcesIntSlice;
    /** Name is unique identifier among all resource instances managed by the driver on the node. It must be a DNS subdomain. */
    name: string;
    /** QuantityValue is a quantity. */
    quantity?: Quantity;
    /** StringValue is a string. */
    string?: string;
    /** StringSliceValue is an array of strings. */
    stringSlice?: NamedResourcesStringSlice;
    /** VersionValue is a semantic version according to semver.org spec 2.0.0. */
    version?: string;
}
/** NamedResourcesFilter is used in ResourceFilterModel. */
export interface NamedResourcesFilter {
    /**
     * Selector is a CEL expression which must evaluate to true if a resource instance is suitable. The language is as defined in https://kubernetes.io/docs/reference/using-api/cel/
     *
     * In addition, for each type NamedResourcesin AttributeValue there is a map that resolves to the corresponding value of the instance under evaluation. For example:
     *
     *    attributes.quantity["a"].isGreaterThan(quantity("0")) &&
     *    attributes.stringslice["b"].isSorted()
     */
    selector: string;
}
/** NamedResourcesInstance represents one individual hardware instance that can be selected based on its attributes. */
export interface NamedResourcesInstance {
    /** Attributes defines the attributes of this resource instance. The name of each attribute must be unique. */
    attributes?: Array<NamedResourcesAttribute>;
    /** Name is unique identifier among all resource instances managed by the driver on the node. It must be a DNS subdomain. */
    name: string;
}
/** NamedResourcesIntSlice contains a slice of 64-bit integers. */
export interface NamedResourcesIntSlice {
    /** Ints is the slice of 64-bit integers. */
    ints: Array<number>;
}
/** NamedResourcesRequest is used in ResourceRequestModel. */
export interface NamedResourcesRequest {
    /**
     * Selector is a CEL expression which must evaluate to true if a resource instance is suitable. The language is as defined in https://kubernetes.io/docs/reference/using-api/cel/
     *
     * In addition, for each type NamedResourcesin AttributeValue there is a map that resolves to the corresponding value of the instance under evaluation. For example:
     *
     *    attributes.quantity["a"].isGreaterThan(quantity("0")) &&
     *    attributes.stringslice["b"].isSorted()
     */
    selector: string;
}
/** NamedResourcesResources is used in ResourceModel. */
export interface NamedResourcesResources {
    /** The list of all individual resources instances currently available. */
    instances: Array<NamedResourcesInstance>;
}
/** NamedResourcesStringSlice contains a slice of strings. */
export interface NamedResourcesStringSlice {
    /** Strings is the slice of strings. */
    strings: Array<string>;
}
/**
 * PodSchedulingContext objects hold information that is needed to schedule a Pod with ResourceClaims that use "WaitForFirstConsumer" allocation mode.
 *
 * This is an alpha type and requires enabling the DynamicResourceAllocation feature gate.
 */
export interface PodSchedulingContext {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "resource.k8s.io/v1alpha2";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "PodSchedulingContext";
    /** Standard object metadata */
    metadata?: ObjectMeta;
    /** Spec describes where resources for the Pod are needed. */
    spec: PodSchedulingContextSpec;
    /** Status describes where resources for the Pod can be allocated. */
    status?: PodSchedulingContextStatus;
}
/** PodSchedulingContextList is a collection of Pod scheduling objects. */
export interface PodSchedulingContextList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "resource.k8s.io/v1alpha2";
    /** Items is the list of PodSchedulingContext objects. */
    items: Array<PodSchedulingContext>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "PodSchedulingContextList";
    /** Standard list metadata */
    metadata?: ListMeta;
}
/** PodSchedulingContextSpec describes where resources for the Pod are needed. */
export interface PodSchedulingContextSpec {
    /**
     * PotentialNodes lists nodes where the Pod might be able to run.
     *
     * The size of this field is limited to 128. This is large enough for many clusters. Larger clusters may need more attempts to find a node that suits all pending resources. This may get increased in the future, but not reduced.
     */
    potentialNodes?: Array<string>;
    /** SelectedNode is the node for which allocation of ResourceClaims that are referenced by the Pod and that use "WaitForFirstConsumer" allocation is to be attempted. */
    selectedNode?: string;
}
/** PodSchedulingContextStatus describes where resources for the Pod can be allocated. */
export interface PodSchedulingContextStatus {
    /** ResourceClaims describes resource availability for each pod.spec.resourceClaim entry where the corresponding ResourceClaim uses "WaitForFirstConsumer" allocation mode. */
    resourceClaims?: Array<ResourceClaimSchedulingStatus>;
}
/**
 * ResourceClaim describes which resources are needed by a resource consumer. Its status tracks whether the resource has been allocated and what the resulting attributes are.
 *
 * This is an alpha type and requires enabling the DynamicResourceAllocation feature gate.
 */
export interface ResourceClaim {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "resource.k8s.io/v1alpha2";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ResourceClaim";
    /** Standard object metadata */
    metadata?: ObjectMeta;
    /** Spec describes the desired attributes of a resource that then needs to be allocated. It can only be set once when creating the ResourceClaim. */
    spec: ResourceClaimSpec;
    /** Status describes whether the resource is available and with which attributes. */
    status?: ResourceClaimStatus;
}
/** ResourceClaimConsumerReference contains enough information to let you locate the consumer of a ResourceClaim. The user must be a resource in the same namespace as the ResourceClaim. */
export interface ResourceClaimConsumerReference {
    /** APIGroup is the group for the resource being referenced. It is empty for the core API. This matches the group in the APIVersion that is used when creating the resources. */
    apiGroup?: string;
    /** Name is the name of resource being referenced. */
    name: string;
    /** Resource is the type of resource being referenced, for example "pods". */
    resource: string;
    /** UID identifies exactly one incarnation of the resource. */
    uid: string;
}
/** ResourceClaimList is a collection of claims. */
export interface ResourceClaimList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "resource.k8s.io/v1alpha2";
    /** Items is the list of resource claims. */
    items: Array<ResourceClaim>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ResourceClaimList";
    /** Standard list metadata */
    metadata?: ListMeta;
}
/** ResourceClaimParameters defines resource requests for a ResourceClaim in an in-tree format understood by Kubernetes. */
export interface ResourceClaimParameters {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "resource.k8s.io/v1alpha2";
    /**
     * DriverRequests describes all resources that are needed for the allocated claim. A single claim may use resources coming from different drivers. For each driver, this array has at most one entry which then may have one or more per-driver requests.
     *
     * May be empty, in which case the claim can always be allocated.
     */
    driverRequests?: Array<DriverRequests>;
    /** If this object was created from some other resource, then this links back to that resource. This field is used to find the in-tree representation of the claim parameters when the parameter reference of the claim refers to some unknown type. */
    generatedFrom?: ResourceClaimParametersReference;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ResourceClaimParameters";
    /** Standard object metadata */
    metadata?: ObjectMeta;
    /** Shareable indicates whether the allocated claim is meant to be shareable by multiple consumers at the same time. */
    shareable?: boolean;
}
/** ResourceClaimParametersList is a collection of ResourceClaimParameters. */
export interface ResourceClaimParametersList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "resource.k8s.io/v1alpha2";
    /** Items is the list of node resource capacity objects. */
    items: Array<ResourceClaimParameters>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ResourceClaimParametersList";
    /** Standard list metadata */
    metadata?: ListMeta;
}
/** ResourceClaimParametersReference contains enough information to let you locate the parameters for a ResourceClaim. The object must be in the same namespace as the ResourceClaim. */
export interface ResourceClaimParametersReference {
    /** APIGroup is the group for the resource being referenced. It is empty for the core API. This matches the group in the APIVersion that is used when creating the resources. */
    apiGroup?: string;
    /** Kind is the type of resource being referenced. This is the same value as in the parameter object's metadata, for example "ConfigMap". */
    kind: string;
    /** Name is the name of resource being referenced. */
    name: string;
}
/** ResourceClaimSchedulingStatus contains information about one particular ResourceClaim with "WaitForFirstConsumer" allocation mode. */
export interface ResourceClaimSchedulingStatus {
    /** Name matches the pod.spec.resourceClaims[*].Name field. */
    name?: string;
    /**
     * UnsuitableNodes lists nodes that the ResourceClaim cannot be allocated for.
     *
     * The size of this field is limited to 128, the same as for PodSchedulingSpec.PotentialNodes. This may get increased in the future, but not reduced.
     */
    unsuitableNodes?: Array<string>;
}
/** ResourceClaimSpec defines how a resource is to be allocated. */
export interface ResourceClaimSpec {
    /** Allocation can start immediately or when a Pod wants to use the resource. "WaitForFirstConsumer" is the default. */
    allocationMode?: string;
    /**
     * ParametersRef references a separate object with arbitrary parameters that will be used by the driver when allocating a resource for the claim.
     *
     * The object must be in the same namespace as the ResourceClaim.
     */
    parametersRef?: ResourceClaimParametersReference;
    /** ResourceClassName references the driver and additional parameters via the name of a ResourceClass that was created as part of the driver deployment. */
    resourceClassName: string;
}
/** ResourceClaimStatus tracks whether the resource has been allocated and what the resulting attributes are. */
export interface ResourceClaimStatus {
    /** Allocation is set by the resource driver once a resource or set of resources has been allocated successfully. If this is not specified, the resources have not been allocated yet. */
    allocation?: AllocationResult;
    /**
     * DeallocationRequested indicates that a ResourceClaim is to be deallocated.
     *
     * The driver then must deallocate this claim and reset the field together with clearing the Allocation field.
     *
     * While DeallocationRequested is set, no new consumers may be added to ReservedFor.
     */
    deallocationRequested?: boolean;
    /** DriverName is a copy of the driver name from the ResourceClass at the time when allocation started. */
    driverName?: string;
    /**
     * ReservedFor indicates which entities are currently allowed to use the claim. A Pod which references a ResourceClaim which is not reserved for that Pod will not be started.
     *
     * There can be at most 32 such reservations. This may get increased in the future, but not reduced.
     */
    reservedFor?: Array<ResourceClaimConsumerReference>;
}
/** ResourceClaimTemplate is used to produce ResourceClaim objects. */
export interface ResourceClaimTemplate {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "resource.k8s.io/v1alpha2";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ResourceClaimTemplate";
    /** Standard object metadata */
    metadata?: ObjectMeta;
    /**
     * Describes the ResourceClaim that is to be generated.
     *
     * This field is immutable. A ResourceClaim will get created by the control plane for a Pod when needed and then not get updated anymore.
     */
    spec: ResourceClaimTemplateSpec;
}
/** ResourceClaimTemplateList is a collection of claim templates. */
export interface ResourceClaimTemplateList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "resource.k8s.io/v1alpha2";
    /** Items is the list of resource claim templates. */
    items: Array<ResourceClaimTemplate>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ResourceClaimTemplateList";
    /** Standard list metadata */
    metadata?: ListMeta;
}
/** ResourceClaimTemplateSpec contains the metadata and fields for a ResourceClaim. */
export interface ResourceClaimTemplateSpec {
    /** ObjectMeta may contain labels and annotations that will be copied into the PVC when creating it. No other fields are allowed and will be rejected during validation. */
    metadata?: ObjectMeta;
    /** Spec for the ResourceClaim. The entire content is copied unchanged into the ResourceClaim that gets created from this template. The same fields as in a ResourceClaim are also valid here. */
    spec: ResourceClaimSpec;
}
/**
 * ResourceClass is used by administrators to influence how resources are allocated.
 *
 * This is an alpha type and requires enabling the DynamicResourceAllocation feature gate.
 */
export interface ResourceClass {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "resource.k8s.io/v1alpha2";
    /**
     * DriverName defines the name of the dynamic resource driver that is used for allocation of a ResourceClaim that uses this class.
     *
     * Resource drivers have a unique name in forward domain order (acme.example.com).
     */
    driverName: string;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ResourceClass";
    /** Standard object metadata */
    metadata?: ObjectMeta;
    /** ParametersRef references an arbitrary separate object that may hold parameters that will be used by the driver when allocating a resource that uses this class. A dynamic resource driver can distinguish between parameters stored here and and those stored in ResourceClaimSpec. */
    parametersRef?: ResourceClassParametersReference;
    /** If and only if allocation of claims using this class is handled via structured parameters, then StructuredParameters must be set to true. */
    structuredParameters?: boolean;
    /**
     * Only nodes matching the selector will be considered by the scheduler when trying to find a Node that fits a Pod when that Pod uses a ResourceClaim that has not been allocated yet.
     *
     * Setting this field is optional. If null, all nodes are candidates.
     */
    suitableNodes?: NodeSelector;
}
/** ResourceClassList is a collection of classes. */
export interface ResourceClassList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "resource.k8s.io/v1alpha2";
    /** Items is the list of resource classes. */
    items: Array<ResourceClass>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ResourceClassList";
    /** Standard list metadata */
    metadata?: ListMeta;
}
/** ResourceClassParameters defines resource requests for a ResourceClass in an in-tree format understood by Kubernetes. */
export interface ResourceClassParameters {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "resource.k8s.io/v1alpha2";
    /** Filters describes additional contraints that must be met when using the class. */
    filters?: Array<ResourceFilter>;
    /** If this object was created from some other resource, then this links back to that resource. This field is used to find the in-tree representation of the class parameters when the parameter reference of the class refers to some unknown type. */
    generatedFrom?: ResourceClassParametersReference;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ResourceClassParameters";
    /** Standard object metadata */
    metadata?: ObjectMeta;
    /** VendorParameters are arbitrary setup parameters for all claims using this class. They are ignored while allocating the claim. There must not be more than one entry per driver. */
    vendorParameters?: Array<VendorParameters>;
}
/** ResourceClassParametersList is a collection of ResourceClassParameters. */
export interface ResourceClassParametersList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "resource.k8s.io/v1alpha2";
    /** Items is the list of node resource capacity objects. */
    items: Array<ResourceClassParameters>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ResourceClassParametersList";
    /** Standard list metadata */
    metadata?: ListMeta;
}
/** ResourceClassParametersReference contains enough information to let you locate the parameters for a ResourceClass. */
export interface ResourceClassParametersReference {
    /** APIGroup is the group for the resource being referenced. It is empty for the core API. This matches the group in the APIVersion that is used when creating the resources. */
    apiGroup?: string;
    /** Kind is the type of resource being referenced. This is the same value as in the parameter object's metadata. */
    kind: string;
    /** Name is the name of resource being referenced. */
    name: string;
    /** Namespace that contains the referenced resource. Must be empty for cluster-scoped resources and non-empty for namespaced resources. */
    namespace?: string;
}
/** ResourceFilter is a filter for resources from one particular driver. */
export interface ResourceFilter {
    /** DriverName is the name used by the DRA driver kubelet plugin. */
    driverName?: string;
    /** NamedResources describes a resource filter using the named resources model. */
    namedResources?: NamedResourcesFilter;
}
/** ResourceHandle holds opaque resource data for processing by a specific kubelet plugin. */
export interface ResourceHandle {
    /**
     * Data contains the opaque data associated with this ResourceHandle. It is set by the controller component of the resource driver whose name matches the DriverName set in the ResourceClaimStatus this ResourceHandle is embedded in. It is set at allocation time and is intended for processing by the kubelet plugin whose name matches the DriverName set in this ResourceHandle.
     *
     * The maximum size of this field is 16KiB. This may get increased in the future, but not reduced.
     */
    data?: string;
    /** DriverName specifies the name of the resource driver whose kubelet plugin should be invoked to process this ResourceHandle's data once it lands on a node. This may differ from the DriverName set in ResourceClaimStatus this ResourceHandle is embedded in. */
    driverName?: string;
    /** If StructuredData is set, then it needs to be used instead of Data. */
    structuredData?: StructuredResourceHandle;
}
/** ResourceRequest is a request for resources from one particular driver. */
export interface ResourceRequest {
    /** NamedResources describes a request for resources with the named resources model. */
    namedResources?: NamedResourcesRequest;
    /** VendorParameters are arbitrary setup parameters for the requested resource. They are ignored while allocating a claim. */
    vendorParameters?: RawExtension;
}
/** ResourceSlice provides information about available resources on individual nodes. */
export interface ResourceSlice {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "resource.k8s.io/v1alpha2";
    /** DriverName identifies the DRA driver providing the capacity information. A field selector can be used to list only ResourceSlice objects with a certain driver name. */
    driverName: string;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ResourceSlice";
    /** Standard object metadata */
    metadata?: ObjectMeta;
    /** NamedResources describes available resources using the named resources model. */
    namedResources?: NamedResourcesResources;
    /**
     * NodeName identifies the node which provides the resources if they are local to a node.
     *
     * A field selector can be used to list only ResourceSlice objects with a certain node name.
     */
    nodeName?: string;
}
/** ResourceSliceList is a collection of ResourceSlices. */
export interface ResourceSliceList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "resource.k8s.io/v1alpha2";
    /** Items is the list of node resource capacity objects. */
    items: Array<ResourceSlice>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "ResourceSliceList";
    /** Standard list metadata */
    metadata?: ListMeta;
}
/** StructuredResourceHandle is the in-tree representation of the allocation result. */
export interface StructuredResourceHandle {
    /** NodeName is the name of the node providing the necessary resources if the resources are local to a node. */
    nodeName?: string;
    /** Results lists all allocated driver resources. */
    results: Array<DriverAllocationResult>;
    /** VendorClaimParameters are the per-claim configuration parameters from the resource claim parameters at the time that the claim was allocated. */
    vendorClaimParameters?: RawExtension;
    /** VendorClassParameters are the per-claim configuration parameters from the resource class at the time that the claim was allocated. */
    vendorClassParameters?: RawExtension;
}
/** VendorParameters are opaque parameters for one particular driver. */
export interface VendorParameters {
    /** DriverName is the name used by the DRA driver kubelet plugin. */
    driverName?: string;
    /** Parameters can be arbitrary setup parameters. They are ignored while allocating a claim. */
    parameters?: RawExtension;
}
