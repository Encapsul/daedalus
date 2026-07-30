import { ObjectMeta } from "../meta/v1";
import { UserInfo } from "./v1";
/** SelfSubjectReview contains the user information that the kube-apiserver has about the user making this request. When using impersonation, users will receive the user info of the user being impersonated.  If impersonation or request header authentication is used, any extra keys will have their case ignored and returned as lowercase. */
export interface SelfSubjectReview {
    /** APIVersion defines the versioned schema of this representation of an object. Servers should convert recognized schemas to the latest internal value, and may reject unrecognized values. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#resources */
    apiVersion?: "authentication.k8s.io/v1alpha1";
    /** Kind is a string value representing the REST resource this object represents. Servers may infer this from the endpoint the client submits requests to. Cannot be updated. In CamelCase. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#types-kinds */
    kind?: "SelfSubjectReview";
    /** Standard object's metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata */
    metadata?: ObjectMeta;
    /** Status is filled in by the server with the user attributes. */
    status?: SelfSubjectReviewStatus;
}
/** SelfSubjectReviewStatus is filled by the kube-apiserver and sent back to a user. */
export interface SelfSubjectReviewStatus {
    /** User attributes of the user making this request. */
    userInfo?: UserInfo;
}
