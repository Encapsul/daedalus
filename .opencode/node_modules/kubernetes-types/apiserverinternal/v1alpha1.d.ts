import { ListMeta, ObjectMeta, Time } from "../meta/v1";
/** An API server instance reports the version it can decode and the version it encodes objects to when persisting objects in the backend. */
export interface ServerStorageVersion {
    /** The ID of the reporting API server. */
    apiServerID?: string;
    /** The API server can decode objects encoded in these versions. The encodingVersion must be included in the decodableVersions. */
    decodableVersions?: Array<string>;
    /** The API server encodes the object to this version when persisting it in the backend (e.g., etcd). */
    encodingVersion?: string;
    /** The API server can serve these versions. DecodableVersions must include all ServedVersions. */
    servedVersions?: Array<string>;
}
/** Storage version of a specific resource. */
export interface StorageVersion {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "internal.apiserver.k8s.io/v1alpha1";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "StorageVersion";
    /** The name is <group>.<resource>. */
    metadata?: ObjectMeta;
    /** Spec is an empty spec. It is here to comply with Kubernetes API style. */
    spec: StorageVersionSpec;
    /** API server instances report the version they can decode and the version they encode objects to when persisting objects in the backend. */
    status: StorageVersionStatus;
}
/** Describes the state of the storageVersion at a certain point. */
export interface StorageVersionCondition {
    /** Last time the condition transitioned from one status to another. */
    lastTransitionTime?: Time;
    /** A human readable message indicating details about the transition. */
    message: string;
    /** If set, this represents the .metadata.generation that the condition was set based upon. */
    observedGeneration?: number;
    /** The reason for the condition's last transition. */
    reason: string;
    /** Status of the condition, one of True, False, Unknown. */
    status: string;
    /** Type of the condition. */
    type: string;
}
/** A list of StorageVersions. */
export interface StorageVersionList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "internal.apiserver.k8s.io/v1alpha1";
    /** Items holds a list of StorageVersion */
    items: Array<StorageVersion>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "StorageVersionList";
    /** Standard list metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ListMeta;
}
/** StorageVersionSpec is an empty spec. */
export interface StorageVersionSpec {
}
/** API server instances report the versions they can decode and the version they encode objects to when persisting objects in the backend. */
export interface StorageVersionStatus {
    /** If all API server instances agree on the same encoding storage version, then this field is set to that version. Otherwise this field is left empty. API servers should finish updating its storageVersionStatus entry before serving write operations, so that this field will be in sync with the reality. */
    commonEncodingVersion?: string;
    /** The latest available observations of the storageVersion's state. */
    conditions?: Array<StorageVersionCondition>;
    /** The reported versions per API server instance. */
    storageVersions?: Array<ServerStorageVersion>;
}
