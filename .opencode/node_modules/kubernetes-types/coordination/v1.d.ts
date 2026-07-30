import { ListMeta, MicroTime, ObjectMeta } from "../meta/v1";
/** Lease defines a lease concept. */
export interface Lease {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "coordination.k8s.io/v1";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "Lease";
    /** More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ObjectMeta;
    /** spec contains the specification of the Lease. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#spec-and-status */
    spec?: LeaseSpec;
}
/** LeaseList is a list of Lease objects. */
export interface LeaseList {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "coordination.k8s.io/v1";
    /** items is a list of schema objects. */
    items: Array<Lease>;
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "LeaseList";
    /** Standard list metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ListMeta;
}
/** LeaseSpec is a specification of a Lease. */
export interface LeaseSpec {
    /** acquireTime is a time when the current lease was acquired. */
    acquireTime?: MicroTime;
    /** holderIdentity contains the identity of the holder of a current lease. */
    holderIdentity?: string;
    /** leaseDurationSeconds is a duration that candidates for a lease need to wait to force acquire it. This is measure against time of last observed renewTime. */
    leaseDurationSeconds?: number;
    /** leaseTransitions is the number of transitions of a lease between holders. */
    leaseTransitions?: number;
    /** renewTime is a time when the current holder of a lease has last updated the lease. */
    renewTime?: MicroTime;
}
