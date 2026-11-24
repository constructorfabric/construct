import { createPrivateKey, sign } from "node:crypto";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { setTimeout as delay } from "node:timers/promises";

const api = "https://api.appstoreconnect.apple.com";

class ReleaseError extends Error {}

function appleClient() {
    const { APP_STORE_CONNECT_KEY_PATH: path, APP_STORE_CONNECT_KEY_ID: kid,
        APP_STORE_CONNECT_ISSUER_ID: iss } = process.env;
    if (!path || !kid || !iss) throw new ReleaseError("The App Store Connect team key is required.");
    const key = createPrivateKey(readFileSync(path));
    if (key.asymmetricKeyDetails?.namedCurve !== "prime256v1") {
        throw new ReleaseError("The App Store Connect key must use P-256.");
    }
    const encode = (value) => Buffer.from(JSON.stringify(value)).toString("base64url");
    return async (route, params = {}) => {
        const url = new URL(route, api);
        if (url.origin !== api || url.username || url.password || !url.pathname.startsWith("/v1/")) {
            throw new ReleaseError("Unexpected App Store Connect API URL.");
        }
        for (const [name, value] of Object.entries(params)) url.searchParams.set(name, value);
        const iat = Math.floor(Date.now() / 1000);
        const message = encode({ alg: "ES256", kid, typ: "JWT" }) + "." +
            encode({ iss, iat, exp: iat + 300, aud: "appstoreconnect-v1" });
        const signature = sign("sha256", Buffer.from(message), { key, dsaEncoding: "ieee-p1363" });
        const response = await fetch(url, {
            headers: { Authorization: "Bearer " + message + "." + signature.toString("base64url") },
            redirect: "error",
            signal: AbortSignal.timeout(20_000),
        });
        if (!response.ok) throw new ReleaseError("App Store Connect returned HTTP " + response.status + ".");
        return response.json();
    };
}

async function* list(request, route, params) {
    for (let page = 0; route; page++) {
        if (page === 50) throw new ReleaseError("Too many App Store Connect results to verify safely.");
        const response = await request(route, params);
        if (!Array.isArray(response.data)) throw new ReleaseError("Invalid App Store Connect list response.");
        yield* response.data;
        route = response.links?.next;
        params = undefined;
    }
}

async function select(items, match) {
    for await (const item of items) {
        if (match(item)) return item;
    }
}

export async function checkTestFlight(request, mode, { bundle, version, build, group }) {
    const app = await select(list(request, "/v1/apps", { "filter[bundleId]": bundle }),
        (item) => item.attributes?.bundleId === bundle);
    if (!app?.id) throw new ReleaseError("The release bundle ID has no App Store Connect app.");

    const groups = list(request, "/v1/betaGroups", { "filter[app]": app.id });
    const internal = await select(groups, (item) =>
        item.attributes?.isInternalGroup === true && item.attributes.name === group);
    if (!internal?.id) throw new ReleaseError("The requested internal TestFlight group does not exist.");

    const builds = list(request, "/v1/builds", { "filter[app]": app.id, "filter[version]": build });
    const existing = await select(builds, (item) => item.attributes?.version === build);
    if (mode === "unused") {
        const uploads = list(request, "/v1/apps/" + encodeURIComponent(app.id) + "/buildUploads",
            { "filter[cfBundleVersion]": build });
        if (existing || await select(uploads, (item) => item.attributes?.cfBundleVersion === build)) {
            throw new ReleaseError("The build number was already uploaded. Commit a new number before releasing.");
        }
        return;
    }

    if (!existing?.id) return false;
    const buildRoute = "/v1/builds/" + encodeURIComponent(existing.id);
    const { data: train } = await request(buildRoute + "/preReleaseVersion");
    if (train?.attributes?.version !== version || train.attributes.platform !== "IOS") {
        throw new ReleaseError("Apple reported the build number for a different version or platform.");
    }
    const processing = existing.attributes.processingState;
    if (processing === "FAILED" || processing === "INVALID" || existing.attributes.expired) {
        throw new ReleaseError("Apple rejected or expired the TestFlight build.");
    }
    if (processing !== "VALID") return false;
    const { data: detail } = await request(buildRoute + "/buildBetaDetail");
    if (detail?.attributes?.internalBuildState !== "IN_BETA_TESTING") return false;
    const groupBuilds = list(request, "/v1/betaGroups/" + encodeURIComponent(internal.id) + "/builds");
    return Boolean(await select(groupBuilds, (item) => item.id === existing.id));
}

async function main() {
    const [mode, bundle, version, build] = process.argv.slice(2);
    const group = process.env.TESTFLIGHT_INTERNAL_GROUP;
    if (!["unused", "verify"].includes(mode) || !bundle || !version || !build || !group) {
        throw new ReleaseError("TestFlight verification requires the mode, bundle, version, build and internal group.");
    }
    const request = appleClient();
    const release = { bundle, version, build, group };
    if (mode === "unused") {
        await checkTestFlight(request, mode, release);
        return;
    }
    const deadline = Date.now() + 30 * 60_000;
    do {
        if (await checkTestFlight(request, mode, release)) {
            console.log("Apple accepted iOS " + version + " (" + build + ") and enabled internal testing.");
            return;
        }
        await delay(30_000);
    } while (Date.now() < deadline);
    throw new ReleaseError("Timed out waiting for Apple processing and internal TestFlight availability. Check App Store Connect before retrying.");
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
    main().catch((error) => {
        console.error(error instanceof ReleaseError ? error.message :
            "App Store Connect verification failed. Check the release inputs and App Store Connect before retrying.");
        process.exitCode = 1;
    });
}
