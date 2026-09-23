# How to investigate failed Salesforce package push upgrades

A Salesforce push upgrade can succeed for some subscribers and fail for others. An email with a failure
count leaves three questions: which orgs failed, what went wrong, and which orgs should receive a retry?

This guide walks through the investigation using the Salesforce CLI and SOQL, then shows how
[sf-cockpit](../README.md) brings those results together. The examples target second-generation managed
and unlocked packages through their owning Dev Hub.

## Before you start

You need the Salesforce CLI and access to the Dev Hub that owns the package. Replace `DevHub` below
with its authenticated alias or username. Replace every ID containing `...` with a complete ID from
your own release; the abbreviated IDs are placeholders, not runnable examples.

The inspection commands below read data. The retry command later in the guide schedules an actual
upgrade in the selected subscriber orgs.

Keep the different IDs straight:

| Prefix | Identifies | Used here for |
|---|---|---|
| `0Ho` | Second-generation package | Selecting the package in sf-cockpit |
| `033` | Subscriber package | Filtering the subscriber query |
| `04t` | Installable package version | Choosing the version to push |
| `0DV` | Push request | Looking up one upgrade attempt |
| `0DX` | Push job | Matching a subscriber's attempt to its errors |
| `00D` | Subscriber org | Matching the customer and selecting retry targets |

sf-cockpit resolves the `0Ho` package to its `033` subscriber package through `Package2`. Its push
requests, jobs, errors, versions, and subscribers are read with the standard Data API. Use
`sf data query` without `--use-tooling-api` for the queries below.

## 1. Find the push upgrade request

If you have the request ID from scheduling the upgrade, start with that. Otherwise, list recent requests
on the Dev Hub:

```bash
sf data query --target-org DevHub --query "SELECT Id, Status, PackageVersionId, ScheduledStartTime, StartTime, EndTime FROM PackagePushRequest ORDER BY ScheduledStartTime DESC NULLS LAST LIMIT 30"
```

Identify the request by its target version and timing. This query returns recent requests across
packages, so check `PackageVersionId` before proceeding. A request's overall status does not tell you
which individual subscribers failed.

You can also inspect a known request using Salesforce's built-in report command:

```bash
sf package push-upgrade report --target-dev-hub DevHub --push-request-id '0DV...'
```

The report includes status and error details where available. SOQL is useful when you want to inspect
the underlying jobs and connect them to subscriber records.

## 2. Find the subscriber orgs that failed

Query the failed jobs for that request:

```bash
sf data query --target-org DevHub --query "SELECT Id, SubscriberOrganizationKey, Status, StartTime, EndTime FROM PackagePushJob WHERE PackagePushRequestId = '0DV...' AND Status = 'Failed'"
```

Keep both the job `Id` and `SubscriberOrganizationKey`. The first connects to error records; the second
identifies the subscriber org. Remove the status filter to inspect all jobs, including those still
waiting or running. An empty failed-job list is not proof that the whole request has finished.

## 3. Read PackagePushError messages

Retrieve the error records associated with the request:

```bash
sf data query --target-org DevHub --query "SELECT PackagePushJobId, ErrorSeverity, ErrorType, ErrorTitle, ErrorMessage, ErrorDetails FROM PackagePushError WHERE PackagePushJob.PackagePushRequestId = '0DV...'"
```

Match `PackagePushError.PackagePushJobId` to `PackagePushJob.Id` from the previous step. Read the full
message and details, not just the error type. Preserve multiple error records for a job rather than
assuming the first record explains everything.

If Salesforce returns only an `UnclassifiedError` or an internal error number, neither this query nor
sf-cockpit can reveal information Salesforce did not return. Keep the request, job, version, and error
identifiers for a support case.

## 4. Match org IDs to subscribers and installed versions

Using the package's `033` subscriber package ID, query its subscribers:

```bash
sf data query --target-org DevHub --query "SELECT OrgKey, OrgName, OrgType, OrgStatus, InstanceName, MetadataPackageVersionId FROM PackageSubscriber WHERE MetadataPackageId = '033...'"
```

Match `SubscriberOrganizationKey` from the failed job to `OrgKey` in the subscriber results. sf-cockpit
normalizes org IDs to their first 15 characters for this comparison; keep their original letter case.

To turn installed version IDs into version numbers, query the same package's versions:

```bash
sf data query --target-org DevHub --query "SELECT Id, Name, MajorVersion, MinorVersion, PatchVersion, BuildNumber, ReleaseState FROM MetadataPackageVersion WHERE MetadataPackageId = '033...'"
```

Match `MetadataPackageVersionId` to the version `Id`. You now have the failed job, its messages, the
subscriber's name and instance, and its current version. This also lets you notice when a subscriber
has already upgraded since the failed attempt.

## 5. Address the cause before retrying

Use the [common push upgrade error reference](../README.md#common-push-upgrade-errors) for
`IneligibleUpgrade`, `UnclassifiedError`, and `ApexTestFailure`.

Treat the message as evidence, not just a reason to repeat the same command. A version-availability
message may call for waiting; a subscriber configuration problem needs investigation in that org;
a defect in the package may require a corrected release.

A manual installation can sometimes provide more detail for an unexpected failure, but it is an
actual installation and can succeed. Coordinate access and the change with the customer before using
that approach in a production org. It does not guarantee a more specific error.

## 6. Retry only the affected subscriber orgs

Recheck the failed orgs and their installed versions after addressing the cause. Build a reviewed list
of orgs that still need the upgrade. Scheduling another request does not repair the original failure
or update the original request in place.

The following command creates a new push request. Replace the version and org IDs before running it;
without a start time it requests execution as soon as resources are available:

```bash
sf package push-upgrade schedule \
  --target-dev-hub DevHub \
  --package '04t...' \
  --org-list '00D...,00D...'
```

Use a released, non-beta version. If you need a maintenance window, add `--start-time` with the intended
UTC date and time, formatted as `YYYY-MM-DDTHH:MM:SS`. Unlike sf-cockpit's wizard, this direct CLI command
should be treated as the execution step, not a preview. Inspect the new request with the report command
and repeat the job/error checks if it fails.

If a scheduling attempt is rejected, inspect its CLI output and any
`job_errors/push_request_<id>_errors.log` file. A request can remain in `Created`; inspect it before
scheduling another attempt. sf-cockpit offers abort for requests in `Created` or `Pending`.

## Do the same investigation in sf-cockpit

sf-cockpit is a free, MIT-licensed terminal app that runs the Salesforce CLI with your existing login.
It combines the records above in its Push Upgrades tab, with failed jobs first, subscriber names,
error details, and troubleshooting hints for known errors.

After [installing sf-cockpit](../README.md#install), explore the workflow with fictional data:

```bash
sf-cockpit --demo
```

For your own package, specify the Dev Hub and `0Ho` package ID:

```bash
sf-cockpit --dev-hub DevHub --package '0Ho...' --tab push
```

1. Refresh with `r` and check the displayed data age before making a retry decision.
2. Select the push request, then a failed org to inspect its errors.
3. After addressing the cause, press `f` to open the retry wizard. It preselects failed orgs from the
   selected request that are present in the subscriber list.
4. Review the target version, org selection, and timing. Check the exact `sf` command on the final
   confirmation screen before approving it.

The Subscribers tab can store your own customer names and important-org markers. For a text summary
of push requests, you can also run `sf-cockpit --print --tab push` with your configured Dev Hub and package.
See the [README](../README.md) for configuration and the remaining release-management features.

## Salesforce references

- [Schedule and inspect push upgrades using the Salesforce CLI](https://developer.salesforce.com/docs/platform/pkg2-dev/guide/push-upgrade-cli.html)
- [Push upgrade scheduling command and flags](https://developer.salesforce.com/docs/platform/salesforce-cli-reference/guide/cli_reference_package_push-upgrade_schedule.html)

When sharing a troubleshooting example publicly, remove customer names, org IDs, and sensitive details
from error messages. Keep the complete identifiers for your private investigation or support case.
