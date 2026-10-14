<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import GenericModal from '@/components/utils/GenericModal.vue'

const props = defineProps<{
    show: boolean
    advertisers: CommercialAdvertiser[]
    campaign: CommercialCampaign | null
}>()
const emit = defineEmits<{
    (event: 'close'): void
    (event: 'save', payload: { campaign: Partial<CommercialCampaign>; newAdvertiser: string }): void
}>()

const { t } = useI18n()

const STEPS = 4

const step = ref(1)
const advertiserId = ref<number | 'new' | null>(null)
const newAdvertiser = ref('')
const name = ref('')
const kind = ref<CampaignKind>('campaign')
const startDate = ref('')
const endDate = ref('')
const maxPerDay = ref<number | null>(null)
const weight = ref(1)
const position = ref<BreakPosition>('any')

const editing = computed(() => props.campaign !== null)
// An emptied number input holds an empty string, which means "no limit".
const limit = computed(() => (typeof maxPerDay.value === 'number' ? maxPerDay.value : null))

watch(
    () => props.show,
    (visible) => {
        if (!visible) {
            return
        }

        const campaign = props.campaign

        step.value = campaign ? 2 : 1
        advertiserId.value = campaign?.advertiser_id ?? null
        newAdvertiser.value = ''
        name.value = campaign?.name ?? ''
        kind.value = campaign?.kind ?? 'campaign'
        startDate.value = campaign?.start_date ?? ''
        endDate.value = campaign?.end_date ?? ''
        maxPerDay.value = campaign?.max_per_day ?? null
        weight.value = campaign?.weight ?? 1
        position.value = campaign?.position ?? 'any'
    },
)

const stepError = computed(() => {
    if (step.value === 1) {
        if (advertiserId.value === null) {
            return t('commercial.wizard.needAdvertiser')
        }

        if (advertiserId.value === 'new' && !newAdvertiser.value.trim()) {
            return t('commercial.wizard.needAdvertiserName')
        }
    }

    if (step.value === 2) {
        if (!name.value.trim()) {
            return t('commercial.wizard.needName')
        }

        if (kind.value === 'campaign') {
            if (!startDate.value || !endDate.value) {
                return t('commercial.wizard.needDates')
            }

            if (startDate.value > endDate.value) {
                return t('commercial.wizard.datesOrder')
            }
        }
    }

    if (step.value === 3 && limit.value !== null && limit.value < 1) {
        return t('commercial.wizard.needLimit')
    }

    return ''
})

function next() {
    if (!stepError.value) {
        step.value = Math.min(STEPS, step.value + 1)
    }
}

function back() {
    step.value = Math.max(editing.value ? 2 : 1, step.value - 1)
}

function save() {
    if (stepError.value || advertiserId.value === null) {
        return
    }

    emit('save', {
        newAdvertiser: advertiserId.value === 'new' ? newAdvertiser.value.trim() : '',
        campaign: {
            id: props.campaign?.id,
            advertiser_id: advertiserId.value === 'new' ? 0 : advertiserId.value,
            name: name.value.trim(),
            kind: kind.value,
            start_date: kind.value === 'campaign' ? startDate.value : null,
            end_date: kind.value === 'campaign' ? endDate.value : null,
            active: true,
            weight: weight.value,
            max_per_day: limit.value,
            position: position.value,
        },
    })
}

const advertiserLabel = computed(() =>
    advertiserId.value === 'new'
        ? newAdvertiser.value
        : (props.advertisers.find((item) => item.id === advertiserId.value)?.name ?? ''),
)
</script>
<template>
    <GenericModal
        :show="show"
        :title="editing ? t('commercial.wizard.editTitle') : t('commercial.wizard.newTitle')"
        :hide-buttons="true"
        :modal-action="() => emit('close')"
    >
        <div class="w-120 max-w-full space-y-4">
            <ul class="steps steps-horizontal w-full text-xs">
                <li v-for="n in STEPS" :key="n" class="step" :class="n <= step && 'step-accent'">
                    {{ t(`commercial.wizard.step${n}`) }}
                </li>
            </ul>

            <div v-if="step === 1" class="space-y-3">
                <p class="text-sm opacity-80">{{ t('commercial.wizard.advertiserHelp') }}</p>
                <select v-model="advertiserId" class="select select-sm w-full">
                    <option :value="null" disabled>{{ t('commercial.choose') }}</option>
                    <option v-for="advertiser in advertisers" :key="advertiser.id" :value="advertiser.id">
                        {{ advertiser.name }}
                    </option>
                    <option value="new">+ {{ t('commercial.wizard.newAdvertiser') }}</option>
                </select>
                <input
                    v-if="advertiserId === 'new'"
                    v-model="newAdvertiser"
                    class="input input-sm w-full"
                    maxlength="100"
                    :placeholder="t('commercial.wizard.newAdvertiserName')"
                />
            </div>

            <div v-else-if="step === 2" class="space-y-3">
                <label class="form-control">
                    <span class="label-text mb-1">{{ t('commercial.wizard.campaignName') }}</span>
                    <input v-model="name" class="input input-sm w-full" maxlength="100" />
                </label>
                <div class="flex gap-4 text-sm">
                    <label class="flex items-center gap-2">
                        <input v-model="kind" type="radio" value="campaign" class="radio radio-sm" />
                        {{ t('commercial.wizard.kindCampaign') }}
                    </label>
                    <label class="flex items-center gap-2">
                        <input v-model="kind" type="radio" value="standalone" class="radio radio-sm" />
                        {{ t('commercial.wizard.kindStandalone') }}
                    </label>
                </div>
                <div v-if="kind === 'campaign'" class="grid grid-cols-2 gap-3">
                    <label class="form-control">
                        <span class="label-text mb-1">{{ t('commercial.wizard.start') }}</span>
                        <input v-model="startDate" type="date" class="input input-sm w-full" />
                    </label>
                    <label class="form-control">
                        <span class="label-text mb-1">{{ t('commercial.wizard.end') }}</span>
                        <input v-model="endDate" type="date" class="input input-sm w-full" />
                    </label>
                </div>
                <p v-else class="text-sm opacity-70">{{ t('commercial.wizard.standaloneHelp') }}</p>
            </div>

            <div v-else-if="step === 3" class="space-y-3">
                <label class="form-control">
                    <span class="label-text mb-1">{{ t('commercial.wizard.perDay') }}</span>
                    <input
                        v-model.number="maxPerDay"
                        type="number"
                        min="1"
                        class="input input-sm w-full"
                        :placeholder="t('commercial.wizard.noLimit')"
                    />
                    <span class="text-xs opacity-60 mt-1">{{ t('commercial.wizard.perDayHelp') }}</span>
                </label>
                <label class="form-control">
                    <span class="label-text mb-1">{{ t('commercial.wizard.position') }}</span>
                    <select v-model="position" class="select select-sm w-full">
                        <option value="any">{{ t('commercial.position.any') }}</option>
                        <option value="first">{{ t('commercial.position.first') }}</option>
                        <option value="last">{{ t('commercial.position.last') }}</option>
                    </select>
                </label>
                <label class="form-control">
                    <span class="label-text mb-1">{{ t('commercial.wizard.weight') }}: {{ weight }}</span>
                    <input v-model.number="weight" type="range" min="1" max="10" class="range range-xs" />
                    <span class="text-xs opacity-60 mt-1">{{ t('commercial.wizard.weightHelp') }}</span>
                </label>
            </div>

            <div v-else class="space-y-2 text-sm">
                <p class="opacity-80">{{ t('commercial.wizard.review') }}</p>
                <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
                    <dt class="opacity-60">{{ t('commercial.wizard.step1') }}</dt>
                    <dd>{{ advertiserLabel }}</dd>
                    <dt class="opacity-60">{{ t('commercial.campaigns.campaign') }}</dt>
                    <dd>{{ name }}</dd>
                    <dt class="opacity-60">{{ t('commercial.campaigns.period') }}</dt>
                    <dd>
                        {{ kind === 'campaign' ? `${startDate} → ${endDate}` : t('commercial.campaigns.noDeadline') }}
                    </dd>
                    <dt class="opacity-60">{{ t('commercial.wizard.perDay') }}</dt>
                    <dd>{{ limit ?? t('commercial.wizard.noLimit') }}</dd>
                    <dt class="opacity-60">{{ t('commercial.wizard.position') }}</dt>
                    <dd>{{ t(`commercial.position.${position}`) }}</dd>
                </dl>
                <p class="opacity-70">{{ t('commercial.wizard.spotsNext') }}</p>
            </div>

            <p v-if="stepError" class="text-error text-sm">{{ stepError }}</p>

            <div class="flex justify-between pt-2">
                <button class="btn btn-sm" @click="emit('close')">{{ t('cancel') }}</button>
                <div class="join">
                    <button v-if="step > (editing ? 2 : 1)" class="btn btn-sm join-item" @click="back">
                        {{ t('commercial.back') }}
                    </button>
                    <button
                        v-if="step < STEPS"
                        class="btn btn-sm btn-accent join-item"
                        :disabled="!!stepError"
                        @click="next"
                    >
                        {{ t('commercial.next') }}
                    </button>
                    <button v-else class="btn btn-sm btn-accent join-item" @click="save">
                        {{ t('commercial.save') }}
                    </button>
                </div>
            </div>
        </div>
    </GenericModal>
</template>
